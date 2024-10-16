use crate::{
    common::{NUM_CHANNELS, RUNTIME},
    model::*,
    plugin::PluginImpl,
};
use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as base64, Engine as _};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, collections::HashSet, num::NonZeroIsize, sync::Arc};
use tokio::sync::{mpsc::UnboundedReceiver, Mutex};
use tracing::{error, info, warn};

static EDITOR: include_dir::Dir = include_dir::include_dir!("$CARGO_MANIFEST_DIR/resources/editor");
pub struct PluginUiImpl {
    raw_window_handle: raw_window_handle::RawWindowHandle,
    window: Arc<wry::WebView>,

    _data_dir: tempfile::TempDir,

    notification_receiver: UnboundedReceiver<UiNotification>,
    response_receiver: UnboundedReceiver<Response>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "payload")]
pub enum UiNotification {
    UpdatePlayingState(bool),
    UpdatePosition(f32),
}

impl PluginUiImpl {
    pub unsafe fn new(handle: usize, plugin: Arc<Mutex<PluginImpl>>) -> Result<Self> {
        let raw_window_handle =
            raw_window_handle::RawWindowHandle::Win32(raw_window_handle::Win32WindowHandle::new(
                NonZeroIsize::new(usize_to_isize(handle))
                    .ok_or_else(|| anyhow::anyhow!("handle is zero"))?,
            ));
        let window_handle = raw_window_handle::WindowHandle::borrow_raw(raw_window_handle);

        let (notification_sender, notification_receiver) = tokio::sync::mpsc::unbounded_channel();
        {
            let mut plugin = RUNTIME.block_on(plugin.lock());
            plugin.notification_sender = Some(notification_sender);
        }

        let (response_sender, response_receiver) =
            tokio::sync::mpsc::unbounded_channel::<Response>();
        let response_sender = Arc::new(response_sender);

        let plugin_ref = Arc::clone(&plugin);

        let temp_dir = tempfile::TempDir::new()?;
        let mut web_context = wry::WebContext::new(Some(temp_dir.path().to_path_buf()));
        let window_builder = wry::WebViewBuilder::new(&window_handle)
            .with_background_color((165, 212, 173, 255))
            .with_web_context(&mut web_context)
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
            gtk::init().unwrap();
        }
        let window = window_builder.build().unwrap();
        let window = Arc::new(window);

        Ok(PluginUiImpl {
            raw_window_handle,
            window,

            _data_dir: temp_dir,

            notification_receiver,
            response_receiver,
        })
    }

    pub fn idle(&mut self) -> Result<()> {
        while let Ok(message) = self.response_receiver.try_recv() {
            let js = PluginUiImpl::response_to_js(&message);
            self.window.evaluate_script(&js)?;
        }

        if let Ok(notification) = self.notification_receiver.try_recv() {
            let js = format!(
                r#"window.onIpcNotification({})"#,
                serde_json::to_string(&notification).unwrap()
            );
            self.window.evaluate_script(&js)?;
        }

        Ok(())
    }

    pub fn get_native_window_handle(&self) -> usize {
        match self.raw_window_handle {
            raw_window_handle::RawWindowHandle::Win32(handle) => isize_to_usize(handle.hwnd.get()),
            _ => 0,
        }
    }

    pub fn set_size(&self, width: usize, height: usize) -> Result<()> {
        self.window.set_bounds(wry::Rect {
            position: winit::dpi::LogicalPosition::new(0.0, 0.0).into(),
            size: winit::dpi::LogicalSize::new(width as f64, height as f64).into(),
        })?;

        Ok(())
    }

    async fn handle_request(
        plugin: Arc<Mutex<PluginImpl>>,
        request: RequestInner,
    ) -> Result<serde_json::Value> {
        let params = Arc::clone(&plugin.lock().await.params);
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
            RequestInner::SetPhrases(phrases) => {
                let mut params = params.write().await;
                params.phrases = phrases.clone();

                let samples = &mut params.voices;
                let missing_voices = phrases
                    .iter()
                    .filter_map(|phrase| {
                        if samples.contains_key(&phrase.voice) {
                            None
                        } else {
                            Some(phrase.voice.clone())
                        }
                    })
                    .collect::<HashSet<_>>();
                let unused_voices = samples
                    .keys()
                    .filter(|voice| !phrases.iter().any(|phrase| phrase.voice == **voice))
                    .cloned()
                    .collect::<HashSet<_>>();
                for audio_hash in unused_voices {
                    samples.remove(&audio_hash);
                }
                if missing_voices.is_empty() {
                    let plugin = Arc::clone(&plugin);
                    tokio::spawn(async move {
                        plugin.lock().await.update_audio_samples(None).await;
                    });
                }
                Ok(serde_json::to_value(SetPhraseResult {
                    missing_voices: missing_voices.into_iter().collect(),
                })?)
            }
            RequestInner::SetVoices(voices) => {
                {
                    let voices_ref = &mut params.write().await.voices;
                    for (audio_hash, sample) in voices {
                        voices_ref.insert(audio_hash, base64.decode(sample)?);
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
            RequestInner::ShowMessageDialog(params) => {
                let dialog = rfd::AsyncMessageDialog::new()
                    .set_title(&params.title)
                    .set_description(&params.message)
                    .set_buttons(rfd::MessageButtons::Ok);
                let dialog = match params.r#type {
                    DialogType::Info => dialog.set_level(rfd::MessageLevel::Info),
                    DialogType::Warning => dialog.set_level(rfd::MessageLevel::Warning),
                    DialogType::Error => dialog.set_level(rfd::MessageLevel::Error),
                    _ => dialog,
                };
                dialog.show().await;

                return Ok(serde_json::Value::Null);
            }
            RequestInner::ShowQuestionDialog(params) => {
                anyhow::ensure!(
                    (1..=3).contains(&params.buttons.len()),
                    "The number of buttons must be 1 to 3"
                );
                let dialog = rfd::AsyncMessageDialog::new()
                    .set_title(&params.title)
                    .set_description(&params.message);
                let dialog = match params.r#type {
                    DialogType::Info => dialog.set_level(rfd::MessageLevel::Info),
                    DialogType::Warning => dialog.set_level(rfd::MessageLevel::Warning),
                    DialogType::Error => dialog.set_level(rfd::MessageLevel::Error),
                    _ => dialog,
                };
                let dialog = dialog.set_buttons(match params.buttons.len() {
                    1 => rfd::MessageButtons::OkCustom(params.buttons[0].clone()),
                    2 => rfd::MessageButtons::OkCancelCustom(
                        params.buttons[0].clone(),
                        params.buttons[1].clone(),
                    ),
                    3 => rfd::MessageButtons::YesNoCancelCustom(
                        params.buttons[0].clone(),
                        params.buttons[1].clone(),
                        params.buttons[2].clone(),
                    ),
                    _ => unreachable!(),
                });
                let result = dialog.show().await;
                let rfd::MessageDialogResult::Custom(custom_text) = result else {
                    anyhow::bail!("Unexpected dialog result: {:?}", result);
                };
                return Ok(serde_json::to_value(
                    params
                        .buttons
                        .iter()
                        .position(|button| button == &custom_text),
                )?);
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
                let num_channels = if params.routing.channel_mode == ChannelMode::Mono {
                    NUM_CHANNELS
                } else {
                    NUM_CHANNELS / 2
                };
                for (i, track_id) in tracks.keys().enumerate() {
                    if !new_channel_index.contains_key(&track_id) {
                        new_channel_index.insert(track_id.clone(), (i % (num_channels as usize)) as u8);
                    }
                }

                params.routing.channel_index = new_channel_index;
                Ok(serde_json::Value::Null)
            }
        }
    }

    fn response_to_js(response: &Response) -> String {
        let response = serde_json::to_string(response).unwrap();

        format!(r#"window.onIpcResponse({})"#, response)
    }
}

pub fn usize_to_isize(value: usize) -> isize {
    if value > isize::MAX as usize {
        (value - isize::MAX as usize - 1) as isize
    } else {
        value as isize
    }
}
pub fn isize_to_usize(value: isize) -> usize {
    if value < 0 {
        (value + isize::MAX + 1) as usize
    } else {
        value as usize
    }
}
