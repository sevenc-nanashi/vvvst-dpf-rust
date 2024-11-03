use crate::{common::RUNTIME, model::*, plugin::PluginImpl};
use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as base64, Engine as _};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    ffi::c_void,
    num::NonZero,
    ptr::NonNull,
    sync::Arc,
};
use tokio::sync::{mpsc::UnboundedReceiver, Mutex};
use tracing::{error, info, warn};

static EDITOR: include_dir::Dir = include_dir::include_dir!("$CARGO_MANIFEST_DIR/resources/editor");
pub struct PluginUiImpl {
    webview: Arc<wry::WebView>,

    _data_dir: tempfile::TempDir,

    notification_receiver: UnboundedReceiver<UiNotification>,
    response_receiver: UnboundedReceiver<Response>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "payload")]
pub enum UiNotification {
    UpdatePlayingState(bool),
}

impl PluginUiImpl {
    pub unsafe fn new(
        handle: usize,
        plugin: Arc<Mutex<PluginImpl>>,
        width: usize,
        height: usize,
        scale_factor: f64,
    ) -> Result<Self> {
        let raw_window_handle = if cfg!(target_os = "windows") {
            raw_window_handle::RawWindowHandle::Win32(raw_window_handle::Win32WindowHandle::new(
                NonZero::new(handle as isize).ok_or_else(|| anyhow::anyhow!("handle is zero"))?,
            ))
        } else if cfg!(target_os = "macos") {
            raw_window_handle::RawWindowHandle::AppKit(raw_window_handle::AppKitWindowHandle::new(
                NonNull::new(handle as *mut c_void)
                    .ok_or_else(|| anyhow::anyhow!("handle is zero"))?
                    .cast(),
            ))
        } else if cfg!(target_os = "linux") {
            raw_window_handle::RawWindowHandle::Xcb(raw_window_handle::XcbWindowHandle::new(
                NonZero::new(handle as u32).ok_or_else(|| anyhow::anyhow!("handle is zero"))?,
            ))
        } else {
            unreachable!()
        };
        let window_handle = raw_window_handle::WindowHandle::borrow_raw(raw_window_handle);

        let (notification_sender, notification_receiver) = tokio::sync::mpsc::unbounded_channel();
        {
            let mut plugin = plugin.blocking_lock();
            plugin.notification_sender = Some(notification_sender);
        }

        let (response_sender, response_receiver) =
            tokio::sync::mpsc::unbounded_channel::<Response>();
        let response_sender = Arc::new(response_sender);

        let plugin_ref = Arc::clone(&plugin);

        let temp_dir = tempfile::TempDir::new()?;
        let mut web_context = wry::WebContext::new(Some(temp_dir.path().to_path_buf()));
        let webview_builder = wry::WebViewBuilder::with_web_context(&mut web_context)
            .with_bounds(wry::Rect {
                position: winit::dpi::LogicalPosition::new(0.0, 0.0).into(),
                size: winit::dpi::LogicalSize::new(width as f64 / scale_factor, height as f64 / scale_factor).into(),
            })
            .with_clipboard(true)
            .with_background_color((165, 212, 173, 255))
            .with_custom_protocol("app".to_string(), |_id, request| {
                let path = request.uri().path();
                EDITOR
                    .get_file(path.trim_start_matches('/'))
                    .map(|file| {
                        info!("serving file: {:?}", file.path());
                        wry::http::Response::builder()
                            .status(200)
                            .header(
                                "Content-Type",
                                mime_guess::from_path(file.path())
                                    .first_or_octet_stream()
                                    .as_ref(),
                            )
                            .body(Cow::Borrowed(file.contents()))
                            .unwrap()
                    })
                    .unwrap_or_else(|| {
                        wry::http::Response::builder()
                            .status(404)
                            .body(Cow::Borrowed(b"" as &[u8]))
                            .unwrap()
                    })
            })
            .with_url(if cfg!(debug_assertions) {
                option_env!("VVVST_DEV_SERVER_URL").unwrap_or("http://localhost:5173")
            } else {
                "app://vvvst.localhost/index.html"
            })
            .with_ipc_handler(move |message| {
                let response_sender = Arc::clone(&response_sender);
                let plugin_ref = Arc::clone(&plugin_ref);
                let message = message.body().to_string();
                RUNTIME.spawn(async move {
                    let value = match serde_json::from_str::<serde_json::Value>(&message) {
                        Ok(value) => value,
                        Err(err) => {
                            error!("failed to parse message: {}", err);
                            return;
                        }
                    };
                    let value = match serde_json::from_value::<Request>(value.clone()) {
                        Ok(value) => value,
                        Err(err) => {
                            // 可能な限りエラーを返してあげる
                            let request_id = value["requestId"].as_u64();
                            if let Some(request_id) = request_id {
                                let response = Response {
                                    request_id: RequestId(request_id as u32),
                                    payload: Err(format!("failed to parse request: {}", err)),
                                };
                                warn!("failed to parse request: {}", err);
                                if let Err(err) = response_sender.send(response) {
                                    error!("failed to send response: {}", err);
                                }
                            } else {
                                error!("failed to parse request: {}", err);
                            }
                            return;
                        }
                    };
                    let result = PluginUiImpl::handle_request(plugin_ref, value.inner).await;
                    let response = Response {
                        request_id: value.request_id,
                        payload: match result {
                            Ok(value) => Ok(value),
                            Err(err) => Err(err.to_string()),
                        },
                    };
                    if let Err(err) = response_sender.send(response) {
                        error!("failed to send response: {}", err);
                    }
                });
            });

        #[cfg(target_os = "linux")]
        {
            gtk::init()?;
        }
        let webview = webview_builder.build_as_child(&window_handle)?;
        let webview = Arc::new(webview);

        Ok(PluginUiImpl {
            webview,

            _data_dir: temp_dir,

            notification_receiver,
            response_receiver,
        })
    }

    pub fn idle(&mut self) -> Result<()> {
        while let Ok(message) = self.response_receiver.try_recv() {
            let response = serde_json::to_string(&message).unwrap();

            self.webview
                .evaluate_script(&format!(r#"window.onIpcResponse({})"#, response))?;
        }

        if let Ok(notification) = self.notification_receiver.try_recv() {
            info!("rust->js notification: {:?}", notification);
            let js = format!(
                r#"window.onIpcNotification({})"#,
                serde_json::to_string(&notification).unwrap()
            );
            self.webview.evaluate_script(&js)?;
        }

        Ok(())
    }

    pub fn set_size(&self, width: usize, height: usize, scale_factor: f64) -> Result<()> {
        let scaled_width = (width as f64 / scale_factor) as u32;
        let scaled_height = (height as f64 / scale_factor) as u32;
        self.webview.set_bounds(wry::Rect {
            position: winit::dpi::LogicalPosition::new(0.0, 0.0).into(),
            size: winit::dpi::LogicalSize::new(scaled_width as f64, scaled_height as f64).into(),
        })?;
        Ok(())
    }

    async fn handle_request(
        plugin: Arc<Mutex<PluginImpl>>,
        request: RequestInner,
    ) -> Result<serde_json::Value> {
        let params = {
            let plugin = plugin.lock().await;
            Arc::clone(&plugin.params)
        };
        match request {
            RequestInner::GetVersion => Ok(serde_json::to_value(env!("CARGO_PKG_VERSION"))?),
            RequestInner::GetProjectName => Ok(serde_json::to_value("VVVST")?),
            RequestInner::GetConfig => {
                // Windows: %APPDATA%/voicevox/config.json
                // macOS: ~/Library/Application Support/voicevox/config.json
                // Linux: ~/.config/voicevox/config.json
                let config_path = if cfg!(target_os = "windows") {
                    let appdata = std::env::var("APPDATA")?;
                    std::path::PathBuf::from(appdata).join("voicevox/config.json")
                } else if cfg!(target_os = "macos") {
                    let home = std::env::var("HOME")?;
                    std::path::PathBuf::from(home)
                        .join("Library/Application Support/voicevox/config.json")
                } else {
                    let home = std::env::var("HOME")?;
                    std::path::PathBuf::from(home).join(".config/voicevox/config.json")
                };

                if !config_path.exists() {
                    return Ok(serde_json::Value::Null);
                }
                let config = tokio::fs::read_to_string(config_path).await?;

                Ok(serde_json::to_value(config)?)
            }
            RequestInner::GetProject => {
                let project = params.read().await.project.clone();
                Ok(serde_json::to_value(project)?)
            }
            RequestInner::SetProject(project) => {
                let mut params = params.write().await;
                params.project = Some(project.clone());
                Ok(serde_json::Value::Null)
            }
            RequestInner::GetVoices => {
                let plugin = plugin.lock().await;
                let encoded_voices = plugin
                    .voice_caches
                    .iter()
                    .map(|(key, value)| (key.clone(), base64.encode(value)))
                    .collect::<HashMap<_, _>>();

                Ok(serde_json::to_value(encoded_voices)?)
            }
            RequestInner::SetPhrases(phrases) => {
                let mut params = params.write().await;
                params.phrases = phrases.clone();

                let voices = &mut params.voices;
                let missing_voices = phrases
                    .iter()
                    .filter_map(|phrase| {
                        if voices.contains_key(&phrase.voice) {
                            None
                        } else {
                            Some(phrase.voice.clone())
                        }
                    })
                    .collect::<HashSet<_>>();
                if missing_voices.is_empty() {
                    let plugin = Arc::clone(&plugin);
                    tokio::spawn(async move {
                        plugin.lock().await.update_audio_samples(None).await;
                    });
                }
                let used_voices = phrases
                    .iter()
                    .map(|phrase| phrase.voice.clone())
                    .collect::<HashSet<_>>();
                voices.retain(|key, _| used_voices.contains(key));
                Ok(serde_json::to_value(SetPhraseResult {
                    missing_voices: missing_voices.into_iter().collect(),
                })?)
            }
            RequestInner::SetVoices(voices) => {
                {
                    let mut plugin = plugin.lock().await;
                    let voices_ref = &mut params.write().await.voices;
                    for (audio_hash, voice) in voices {
                        let decoded = base64.decode(voice)?;
                        voices_ref.insert(audio_hash.clone(), decoded.clone());
                        plugin.voice_caches.insert(audio_hash, decoded);
                    }
                }

                let plugin = Arc::clone(&plugin);
                tokio::spawn(async move {
                    plugin.lock().await.update_audio_samples(None).await;
                });
                Ok(serde_json::Value::Null)
            }
            RequestInner::ShowImportFileDialog(params) => {
                let dialog = match &params {
                    ShowImportFileDialog {
                        title,
                        name: Some(name),
                        filters: Some(filters),
                    } => rfd::AsyncFileDialog::new()
                        .set_title(title)
                        .add_filter(name, filters),
                    ShowImportFileDialog { title, .. } => {
                        rfd::AsyncFileDialog::new().set_title(title)
                    }
                };

                let result = dialog.pick_file().await;
                return Ok(serde_json::to_value(
                    result.map(|path| path.path().to_string_lossy().to_string()),
                )?);
            }
            RequestInner::ReadFile(path) => {
                let content = tokio::fs::read(path).await?;
                let encoded = base64.encode(&content);
                Ok(serde_json::to_value(encoded)?)
            }
            RequestInner::ExportProject => {
                let destination = rfd::AsyncFileDialog::new()
                    .set_title("プロジェクトファイルの書き出し")
                    .add_filter("VOICEVOX Project File", &["vvproj"])
                    .save_file()
                    .await;
                if let Some(destination) = destination {
                    let params = params.read().await;
                    let project = params.project.clone().unwrap();
                    tokio::fs::write(destination.path(), project).await?;
                    return Ok(serde_json::Value::Bool(true));
                } else {
                    return Ok(serde_json::Value::Bool(false));
                }
            }

            RequestInner::GetRouting => {
                let routing = params.read().await.routing.clone();
                Ok(serde_json::to_value(routing)?)
            }

            RequestInner::SetRouting(routing) => {
                let mut params = params.write().await;
                params.routing = routing.clone();
                Ok(serde_json::Value::Null)
            }

            RequestInner::SetTracks(tracks) => {
                let mut params = params.write().await;
                params.tracks = tracks.clone();
                let mut new_channel_index = params.routing.channel_index.clone();
                new_channel_index.retain(|track_id, _index| tracks.contains_key(&track_id));
                for track_id in tracks.keys() {
                    if !new_channel_index.contains_key(&track_id) {
                        new_channel_index.insert(track_id.clone(), 0);
                    }
                }

                params.routing.channel_index = new_channel_index;
                Ok(serde_json::Value::Null)
            }

            RequestInner::GetCurrentPosition => {
                let mut plugin = plugin.lock().await;
                if plugin.current_position_updated {
                    plugin.current_position_updated = false;
                    Ok(serde_json::to_value(plugin.current_position)?)
                } else {
                    Ok(serde_json::Value::Null)
                }
            }
        }
    }
}
