use crate::{common::RUNTIME, model::*, plugin::PluginImpl};
use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as base64, Engine as _};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    ffi::c_void,
    sync::{Arc, Mutex as SyncMutex},
};
use tokio::sync::{mpsc::UnboundedReceiver, Mutex};
use tracing::{error, info, warn};

static EDITOR: include_dir::Dir = include_dir::include_dir!("$CARGO_MANIFEST_DIR/resources/editor");
pub struct PluginUiImpl {
    window_handle: baseview::WindowHandle,

    resize_request: Arc<SyncMutex<Option<(usize, usize)>>>,
}

struct WebViewWindowHandler {
    webview: Arc<wry::WebView>,
    _data_dir: tempfile::TempDir,

    response_receiver: UnboundedReceiver<Response>,
    notification_receiver: UnboundedReceiver<UiNotification>,
    resize_request: Arc<SyncMutex<Option<(usize, usize)>>>,
}
impl baseview::WindowHandler for WebViewWindowHandler {
    fn on_frame(&mut self, window: &mut baseview::Window) {
        while let Ok(message) = self.response_receiver.try_recv() {
            let response = serde_json::to_string(&message).unwrap();

            if let Err(err) = self
                .webview
                .evaluate_script(&format!(r#"window.onIpcResponse({})"#, response))
            {
                error!("failed to send response: {}", err);
            }
        }

        if let Ok(notification) = self.notification_receiver.try_recv() {
            info!("rust->js notification: {:?}", notification);
            let js = format!(
                r#"window.onIpcNotification({})"#,
                serde_json::to_string(&notification).unwrap()
            );
            if let Err(err) = self.webview.evaluate_script(&js) {
                error!("failed to send notification: {}", err);
            }
        }

        if let Some((width, height)) = self.resize_request.lock().unwrap().take() {
            window.resize(baseview::Size::new(width as f64, height as f64));
        }
    }
    fn on_event(
        &mut self,
        _window: &mut baseview::Window,
        event: baseview::Event,
    ) -> baseview::EventStatus {
        match event {
            baseview::Event::Window(baseview::WindowEvent::Resized(size)) => {
                self.webview.set_bounds(wry::Rect {
                    x: 0,
                    y: 0,
                    width: size.logical_size().width as _,
                    height: size.logical_size().height as _,
                });
                baseview::EventStatus::Captured
            }
            _ => baseview::EventStatus::Ignored,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "payload")]
pub enum UiNotification {
    UpdatePlayingState(bool),
}

struct WindowHandle {
    handle: usize,
}
unsafe impl raw_window_handle::HasRawWindowHandle for WindowHandle {
    fn raw_window_handle(&self) -> raw_window_handle::RawWindowHandle {
        if cfg!(target_os = "windows") {
            let mut rwh = raw_window_handle::Win32WindowHandle::empty();
            rwh.hwnd = self.handle as *mut c_void;
            raw_window_handle::RawWindowHandle::Win32(rwh)
        } else if cfg!(target_os = "macos") {
            let mut rwh = raw_window_handle::AppKitWindowHandle::empty();
            rwh.ns_view = self.handle as *mut c_void;
            raw_window_handle::RawWindowHandle::AppKit(rwh)
        } else if cfg!(target_os = "linux") {
            let mut rwh = raw_window_handle::XcbWindowHandle::empty();
            rwh.window = self.handle as _;
            raw_window_handle::RawWindowHandle::Xcb(rwh)
        } else {
            unreachable!()
        }
    }
}

impl PluginUiImpl {
    pub unsafe fn new(handle: usize, plugin: Arc<Mutex<PluginImpl>>) -> Result<Self> {
        let raw_window_handle = WindowHandle { handle };

        let (notification_sender, notification_receiver) = tokio::sync::mpsc::unbounded_channel();
        {
            let mut plugin = plugin.blocking_lock();
            plugin.notification_sender = Some(notification_sender);
        }

        let (response_sender, response_receiver) =
            tokio::sync::mpsc::unbounded_channel::<Response>();
        let response_sender = Arc::new(response_sender);

        let resize_request = Arc::new(SyncMutex::new(None));
        let resize_request_ref = Arc::clone(&resize_request);

        let plugin_ref = Arc::clone(&plugin);

        #[cfg(target_os = "linux")]
        {
            gtk::init()?;
        }

        let window_handle = baseview::Window::open_parented(
            &raw_window_handle,
            baseview::WindowOpenOptions {
                title: "VVVST".to_string(),
                size: baseview::Size::new(800.0, 600.0),
                scale: baseview::WindowScalePolicy::SystemScaleFactor,
            },
            move |window| {
                let temp_dir = tempfile::TempDir::new().unwrap();
                let mut web_context = wry::WebContext::new(Some(temp_dir.path().to_path_buf()));
                let webview = wry::WebViewBuilder::new_as_child(window)
                    .with_web_context(&mut web_context)
                    .with_clipboard(true)
                    .with_background_color((165, 212, 173, 255))
                    .with_custom_protocol("app".to_string(), |request| {
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
                    .with_ipc_handler(move |message| {
                        let response_sender = Arc::clone(&response_sender);
                        let plugin_ref = Arc::clone(&plugin_ref);
                        let message = message.to_string();
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
                                            payload: Err(format!(
                                                "failed to parse request: {}",
                                                err
                                            )),
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
                            let result =
                                PluginUiImpl::handle_request(plugin_ref, value.inner).await;
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
                    })
                    .with_url(if cfg!(debug_assertions) {
                        option_env!("VVVST_DEV_SERVER_URL").unwrap_or("http://localhost:5173")
                    } else {
                        "app://vvvst.localhost/index.html"
                    })
                    .unwrap()
                    .build()
                    .unwrap();

                let webview = Arc::new(webview);

                let handler = WebViewWindowHandler {
                    webview: Arc::clone(&webview),
                    _data_dir: temp_dir,
                    response_receiver,
                    notification_receiver,
                    resize_request: resize_request_ref,
                };
                handler
            },
        );
        Ok(PluginUiImpl {
            window_handle,
            resize_request,
        })
    }

    pub fn idle(&mut self) -> Result<()> {
        Ok(())
    }

    pub fn set_size(&self, width: usize, height: usize) -> Result<()> {
        *self.resize_request.lock().unwrap() = Some((width, height));
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

impl Drop for PluginUiImpl {
    fn drop(&mut self) {
        self.window_handle.close();
    }
}
