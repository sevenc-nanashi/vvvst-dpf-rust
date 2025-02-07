use crate::{common, ipc_model::*, manager, plugin::PluginImpl, voice::Voice, vst_common::RUNTIME};
use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as base64, Engine as _};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    ffi::c_void,
    num::NonZero,
    ptr::NonNull,
    sync::{Arc, LazyLock},
};
use tap::prelude::*;
use tokio::{
    io::AsyncBufReadExt,
    sync::{
        mpsc::{UnboundedReceiver, UnboundedSender},
        Mutex,
    },
};
use tracing::{error, info, warn};
use uuid::Uuid;

static WEBRTC_API: LazyLock<webrtc::api::API> = LazyLock::new(|| {
    let mut m = webrtc::api::media_engine::MediaEngine::default();

    m.register_codec(
        webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecParameters {
            capability: webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability {
                mime_type: webrtc::api::media_engine::MIME_TYPE_OPUS.to_owned(),
                clock_rate: 48000,
                channels: 2,
                sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
                rtcp_feedback: vec![],
            },
            payload_type: 111,
            ..Default::default()
        },
        webrtc::rtp_transceiver::rtp_codec::RTPCodecType::Audio,
    )
    .expect("failed to register codec");

    let api = webrtc::api::APIBuilder::new().with_media_engine(m).build();

    api
});

static EDITOR: include_dir::Dir = include_dir::include_dir!("$CARGO_MANIFEST_DIR/resources/editor");
pub struct PluginUiImpl {
    webview: Arc<wry::WebView>,

    notification_receiver: UnboundedReceiver<UiNotification>,
    response_receiver: UnboundedReceiver<Response>,

    manager: tokio::task::JoinHandle<()>,
    manager_sender: UnboundedSender<ManagerMessage>,

    zoom_receiver: UnboundedReceiver<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "payload")]
pub enum UiNotification {
    UpdatePlayingState(bool),
    EngineReady { port: u16 },

    RtcIce(serde_json::Value),
}

#[derive(Debug, Clone)]
pub enum ManagerMessage {
    Send(manager::ToManagerMessage),
    Stop,
}

impl PluginUiImpl {
    pub unsafe fn new(
        handle: usize,
        plugin: Arc<Mutex<PluginImpl>>,
        width: usize,
        height: usize,
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
        let (rtc_sender, rtc_receiver) = tokio::sync::mpsc::unbounded_channel();
        {
            let mut plugin = plugin.blocking_lock();
            plugin.notification_sender = Some(notification_sender.clone());
            plugin.rtc_samples = Some(rtc_receiver);
        };

        let notification_sender = Arc::new(notification_sender);
        let rtc_sender = Arc::new(rtc_sender);

        let peer_connection = Arc::new(Mutex::<
            Option<(Uuid, webrtc::peer_connection::RTCPeerConnection)>,
        >::new(None));

        let (manager_sender, mut manager_receiver) = tokio::sync::mpsc::unbounded_channel();

        let manager = RUNTIME
            .lock()
            .unwrap()
            .as_ref()
            .expect("Already dropped")
            .spawn({
                let notification_sender = notification_sender.clone();

                async move {
                    let manager_name = if cfg!(target_os = "windows") {
                        "engine-manager.exe"
                    } else {
                        "engine-manager"
                    };
                    let manager_path = process_path::get_dylib_path()
                        .unwrap()
                        .parent()
                        .unwrap()
                        .join(manager_name);
                    info!("engine-manager path: {:?}", manager_path);
                    let mut manager_process = tokio::process::Command::new(manager_path)
                        .arg(handle.to_string())
                        .stdin(std::process::Stdio::piped())
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::piped())
                        .pipe(|cmd| {
                            #[cfg(target_os = "windows")]
                            let cmd = cmd.creation_flags(common::WINDOWS_CREATE_NO_WINDOW);
                            cmd
                        })
                        .spawn()
                        .unwrap();
                    info!("engine-manager started: {:?}", &manager_process);
                    let stderr = manager_process.stderr.take().unwrap();
                    let stderr = tokio::io::BufReader::new(stderr);
                    tokio::spawn(async move {
                        let mut lines = stderr.lines();
                        while let Some(line) = lines.next_line().await.unwrap() {
                            error!("engine-manager stderr: {:?}", line);
                        }
                    });
                    let port = tokio::io::BufReader::new(manager_process.stdout.as_mut().unwrap())
                        .lines()
                        .next_line()
                        .await
                        .expect("failed to read port")
                        .inspect(|line| info!("engine-manager stdout: {:?}", line))
                        .expect("failed to read port")
                        .parse::<u16>()
                        .unwrap();
                    info!("engine-manager port: {}", port);
                    let mut manager_connection =
                        tokio::net::TcpStream::connect(("127.0.0.1", port))
                            .await
                            .unwrap();
                    manager::pack(manager::ToManagerMessage::Hello, &mut manager_connection)
                        .await
                        .unwrap();
                    let (reader, writer) = manager_connection.into_split();
                    let writer = Arc::new(Mutex::new(writer));
                    let manager_communication = async {
                        let mut reader = tokio::io::BufReader::new(reader);
                        loop {
                            let message = match manager::unpack(&mut reader).await {
                                Ok(message) => message,
                                Err(err) => {
                                    error!("failed to read message: {}", err);
                                    break Err::<(), _>(err);
                                }
                            };
                            match message {
                                manager::ToClientMessage::Hello => {
                                    info!("received hello from engine-manager");
                                }
                                manager::ToClientMessage::Pong => {
                                    // noop
                                }
                                manager::ToClientMessage::EnginePort(port) => {
                                    info!("received engine ready from engine-manager: {}", port);
                                    notification_sender
                                        .send(UiNotification::EngineReady { port })
                                        .map_err(|_| {
                                            anyhow::anyhow!("failed to send engine ready")
                                        })?;
                                }
                            }
                        }
                    };
                    let ping = {
                        let writer = Arc::clone(&writer);
                        async move {
                            loop {
                                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                let writer = &mut *writer.lock().await;
                                if let Err(e) =
                                    manager::pack(manager::ToManagerMessage::Ping, writer).await
                                {
                                    error!("failed to send ping: {}", e);
                                    break Err::<(), _>(e);
                                }
                            }
                        }
                    };
                    let manager_sender_communication = {
                        async {
                            loop {
                                let message = match manager_receiver.recv().await {
                                    Some(message) => message,
                                    None => break Ok(()),
                                };
                                match message {
                                    ManagerMessage::Send(message) => {
                                        info!("sending message to engine-manager: {:?}", message);
                                        let writer = &mut *writer.lock().await;
                                        if let Err(err) = manager::pack(message, writer).await {
                                            error!("failed to send start message: {}", err);
                                            break Err(err);
                                        }
                                    }
                                    ManagerMessage::Stop => {
                                        break Ok(());
                                    }
                                }
                            }
                        }
                    };
                    let result = tokio::select! {
                        result = manager_communication => {
                            result
                        }
                        result = manager_sender_communication => {
                            result
                        }
                        result = ping => {
                            result
                        }
                    };

                    if let Err(err) = result {
                        error!("engine manager communication failed: {}", err);
                    }

                    info!("engine manager connection closed");
                }
            });

        let (response_sender, response_receiver) =
            tokio::sync::mpsc::unbounded_channel::<Response>();
        let response_sender = Arc::new(response_sender);

        let (zoom_sender, zoom_receiver) = tokio::sync::mpsc::unbounded_channel();

        let plugin_ref = Arc::clone(&plugin);

        let mut web_context = wry::WebContext::new(Some(common::data_dir().join("webview_cache")));
        let webview_builder = wry::WebViewBuilder::with_web_context(&mut web_context)
            .with_bounds(wry::Rect {
                position: winit::dpi::LogicalPosition::new(0.0, 0.0).into(),
                size: winit::dpi::PhysicalSize::new(width as f64, height as f64).into(),
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
            .with_url({
                let base_url = if cfg!(debug_assertions) {
                    option_env!("VVVST_DEV_SERVER_URL").unwrap_or("http://localhost:5173")
                } else {
                    "app://vvvst.localhost/index.html"
                };
                format!("{}?engineStatus=notRunning", base_url)
            })
            .with_ipc_handler({
                let manager_sender = manager_sender.clone();
                move |message| {
                    let response_sender = Arc::clone(&response_sender);
                    let plugin_ref = Arc::clone(&plugin_ref);
                    let message = message.body().to_string();
                    let manager_sender = manager_sender.clone();
                    let zoom_sender = zoom_sender.clone();
                    let notification_sender = notification_sender.clone();
                    let peer_connection = Arc::clone(&peer_connection);
                    let rtc_sender = Arc::clone(&rtc_sender);
                    RUNTIME
                        .lock()
                        .unwrap()
                        .as_ref()
                        .expect("Already dropped")
                        .spawn(async move {
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
                            let result = PluginUiImpl::handle_request(
                                plugin_ref,
                                manager_sender,
                                zoom_sender,
                                notification_sender,
                                peer_connection,
                                rtc_sender,
                                value.inner,
                            )
                            .await;
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
                }
            });

        #[cfg(target_os = "linux")]
        {
            gtk::init()?;
        }
        let webview = webview_builder.build_as_child(&window_handle)?;
        let webview = Arc::new(webview);

        Ok(PluginUiImpl {
            webview,

            manager,
            manager_sender,
            notification_receiver,
            response_receiver,
            zoom_receiver,
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
                r#"
                (async () => {{
                    const notification = {};
                    while (true) {{
                        if (window.onIpcNotification != null) {{
                            break;
                        }}
                        await new Promise(resolve => setTimeout(resolve, 0));
                    }}
                    window.onIpcNotification(notification);
                }})();
                "#,
                serde_json::to_string(&notification).unwrap()
            );
            self.webview.evaluate_script(&js)?;
        }

        while let Ok(zoom) = self.zoom_receiver.try_recv() {
            self.webview.zoom(zoom)?;
        }

        Ok(())
    }

    pub fn set_size(&self, width: usize, height: usize) -> Result<()> {
        self.webview.set_bounds(wry::Rect {
            position: winit::dpi::LogicalPosition::new(0.0, 0.0).into(),
            size: winit::dpi::PhysicalSize::new(width as f64, height as f64).into(),
        })?;
        Ok(())
    }

    async fn handle_request(
        plugin: Arc<Mutex<PluginImpl>>,
        manager_sender: UnboundedSender<ManagerMessage>,
        zoom_sender: UnboundedSender<f64>,
        notification_sender: Arc<tokio::sync::mpsc::UnboundedSender<UiNotification>>,
        peer_connection: Arc<Mutex<Option<(Uuid, webrtc::peer_connection::RTCPeerConnection)>>>,
        rtc_sender: Arc<tokio::sync::mpsc::UnboundedSender<Vec<(f32, f32)>>>,
        request: RequestInner,
    ) -> Result<serde_json::Value> {
        let (params, critical_params) = {
            let plugin = plugin.lock().await;
            (
                Arc::clone(&plugin.params),
                Arc::clone(&plugin.critical_params),
            )
        };
        match request {
            RequestInner::GetVersion => Ok(serde_json::to_value(env!("CARGO_PKG_VERSION"))?),
            RequestInner::GetProjectName => Ok(serde_json::to_value("VOICEVOX")?),
            RequestInner::GetConfig => {
                let config = tokio::fs::read_to_string(if editor_config_path().exists() {
                    editor_config_path()
                } else if original_config_path().exists() {
                    original_config_path()
                } else {
                    return Ok(serde_json::Value::Null);
                })
                .await?;

                Ok(serde_json::to_value(config)?)
            }
            RequestInner::SetConfig(config) => {
                let config_path = editor_config_path();
                tokio::fs::write(&config_path, config).await?;
                Ok(serde_json::Value::Null)
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
                    .params
                    .read()
                    .await
                    .voices
                    .iter()
                    .map(|(key, value)| (key.clone(), base64.encode(value.to_vec())))
                    .collect::<HashMap<_, _>>();

                Ok(serde_json::to_value(encoded_voices)?)
            }
            RequestInner::SetPhrases(phrases) => {
                let mut params = params.write().await;
                params.phrases = phrases.iter().cloned().collect();

                let voices = &mut params.voices;
                let missing_voices = phrases
                    .iter()
                    .filter_map(|phrase| {
                        phrase.voice.as_ref().and_then(|voice| {
                            if voices.contains_key(voice) {
                                None
                            } else {
                                Some(voice.clone())
                            }
                        })
                    })
                    .collect::<HashSet<_>>();
                if missing_voices.is_empty() {
                    tokio::spawn(async move {
                        PluginImpl::update_audio_samples(plugin, None).await;
                    });
                }
                let used_voices = phrases
                    .iter()
                    .filter_map(|phrase| phrase.voice.clone())
                    .collect::<HashSet<_>>();
                voices.retain(|key, _| used_voices.contains(key));
                Ok(serde_json::to_value(SetPhraseResult {
                    missing_voices: missing_voices.into_iter().collect(),
                })?)
            }
            RequestInner::SetVoices(voices) => {
                let voices = voices
                    .into_iter()
                    .map(|(key, value)| {
                        base64
                            .decode(value)
                            .map(|value| (key, value))
                            .map_err(anyhow::Error::from)
                    })
                    .collect::<Result<HashMap<_, _>>>()?;
                {
                    let voices_ref = &mut params.write().await.voices;
                    for (audio_hash, voice) in voices {
                        voices_ref.insert(audio_hash, Voice::new(voice)?);
                    }
                }

                let plugin = Arc::clone(&plugin);
                tokio::spawn(async move {
                    PluginImpl::update_audio_samples(plugin, None).await;
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
            RequestInner::WriteFile { path, data } => {
                let content = base64.decode(data)?;
                tokio::fs::write(path, content).await?;
                Ok(serde_json::Value::Null)
            }
            RequestInner::CheckFileExists(path) => {
                let exists = tokio::fs::metadata(path).await.is_ok();
                Ok(serde_json::to_value(exists)?)
            }
            RequestInner::ShowExportFileDialog {
                title,
                default_path,
                extension_name,
                extensions,
            } => {
                let mut dialog = rfd::AsyncFileDialog::new()
                    .set_title(title)
                    .add_filter(extension_name, &extensions);
                if let Some(default_path) = default_path {
                    // default_pathはdefault_nameみたいな名前であるべき
                    // （TODO: 本家を巻き込んで修正）
                    dialog = dialog.set_file_name(default_path);
                }
                let result = dialog.save_file().await;

                return Ok(serde_json::to_value(
                    result.map(|path| path.path().to_string_lossy().to_string()),
                )?);
            }
            RequestInner::ShowSaveDirectoryDialog { title } => {
                let dialog = rfd::AsyncFileDialog::new().set_title(title);
                let result = dialog.pick_folder().await;

                return Ok(serde_json::to_value(
                    result.map(|path| path.path().to_string_lossy().to_string()),
                )?);
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
                let routing = critical_params.read().await.routing.clone();
                Ok(serde_json::to_value(routing)?)
            }

            RequestInner::SetRouting(routing) => {
                let mut params = critical_params.write().await;
                params.routing = routing.clone();
                Ok(serde_json::Value::Null)
            }

            RequestInner::SetTracks(tracks) => {
                let mut params = critical_params.write().await;
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
                let mut plugin_guard = plugin.lock().await;
                if plugin_guard.current_position_updated {
                    plugin_guard.current_position_updated = false;
                    Ok(serde_json::to_value(plugin_guard.current_position)?)
                } else {
                    Ok(serde_json::Value::Null)
                }
            }
            RequestInner::Zoom(value) => {
                zoom_sender
                    .send(value)
                    .map_err(|_| anyhow::anyhow!("failed to send zoom"))?;
                Ok(serde_json::Value::Null)
            }
            RequestInner::StartEngine {
                use_gpu,
                force_restart,
            } => {
                manager_sender
                    .send(ManagerMessage::Send(manager::ToManagerMessage::Start {
                        use_gpu,
                        force_restart,
                    }))
                    .map_err(|_| anyhow::anyhow!("failed to send start message"))?;
                Ok(serde_json::Value::Null)
            }
            RequestInner::ChangeEnginePath => {
                manager_sender
                    .send(ManagerMessage::Send(
                        manager::ToManagerMessage::ChangeEnginePath,
                    ))
                    .map_err(|_| anyhow::anyhow!("failed to send change engine path message"))?;
                Ok(serde_json::Value::Null)
            }
            RequestInner::RtcSdp(sdp) => {
                let new_peer_connection =
                    WEBRTC_API.new_peer_connection(Default::default()).await?;
                new_peer_connection
                    .add_transceiver_from_kind(
                        webrtc::rtp_transceiver::rtp_codec::RTPCodecType::Audio,
                        None,
                    )
                    .await?;
                let nonce = uuid::Uuid::new_v4();
                new_peer_connection.on_peer_connection_state_change(Box::new({
                    let peer_connection = peer_connection.clone();
                    move |state| {
                        info!("peer connection state change: {:?}", state);

                        Box::pin({
                            let peer_connection = peer_connection.clone();
                            async move {
                                if state
                                    == webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState::Failed
                                {
                                    let mut peer_connection = peer_connection.lock().await;
                                    *peer_connection = None;
                                }
                            }
                        })
                    }
                }));

                new_peer_connection.on_ice_candidate(Box::new(move |candidate| {
                    let candidate = candidate.and_then(|candidate| candidate.to_json().ok());
                    if let Err(err) = notification_sender.send(UiNotification::RtcIce(
                        serde_json::to_value(candidate).expect("failed to serialize ice candidate"),
                    )) {
                        error!("failed to send ice candidate: {}", err);
                    }
                    Box::pin(async move {})
                }));
                new_peer_connection.on_track(Box::new(move |track, _, _| {
                    info!("received track: {:?}", track);

                    Box::pin({
                        let rtc_sender = Arc::clone(&rtc_sender);
                        let plugin = Arc::clone(&plugin);
                        async move {
                            {
                                let mut plugin = plugin.lock().await;
                                plugin.rtc_sample_rate =
                                    Some(track.codec().capability.clock_rate as f32);
                            }

                            let mut decoder = match opus::Decoder::new(
                                track.codec().capability.clock_rate as u32,
                                opus::Channels::Stereo,
                            ) {
                                Ok(decoder) => decoder,
                                Err(err) => {
                                    error!("failed to create opus decoder: {}", err);
                                    return;
                                }
                            };

                            info!("start reading rtp");

                            let mut samples = vec![0.0; 5760];
                            while let Ok((packet, _params)) = track.read_rtp().await {
                                let payload = packet.payload;

                                let decoded =
                                    match decoder.decode_float(&payload, &mut samples, false) {
                                        Ok(decoded) => decoded,
                                        Err(err) => {
                                            error!("failed to decode samples: {}", err);
                                            continue;
                                        }
                                    };

                                if let Err(err) = rtc_sender.send(
                                    samples
                                        .iter()
                                        .take(decoded * 2)
                                        .chunks(2)
                                        .into_iter()
                                        .map(|mut chunk| {
                                            (
                                                chunk.next().unwrap().to_owned(),
                                                chunk.next().unwrap().to_owned(),
                                            )
                                        })
                                        .collect(),
                                ) {
                                    error!("failed to send samples: {}", err);
                                    break;
                                }
                            }
                        }
                    })
                }));

                info!("set remote description");
                new_peer_connection
                    .set_remote_description(serde_json::from_value(sdp)?)
                    .await?;

                let answer = new_peer_connection.create_answer(None).await?;
                let answer_value = serde_json::to_value(&answer)?;
                info!("set local description");
                new_peer_connection.set_local_description(answer).await?;

                let mut peer_connection = peer_connection.lock().await;
                *peer_connection = Some((nonce, new_peer_connection));

                let ret = serde_json::json!({
                    "answer": answer_value,
                    "nonce": nonce.to_string(),
                });
                Ok(ret)
            }
            RequestInner::RtcIce { nonce, ice } => {
                let mut peer_connection = peer_connection.lock().await;
                if let (Some((current_nonce, peer_connection)), Some(ice)) =
                    (&mut *peer_connection, ice.as_ref())
                {
                    if *current_nonce == nonce {
                        peer_connection
                            .add_ice_candidate(serde_json::from_value(ice.clone())?)
                            .await?;
                        info!("added ice candidate: {:?}", ice);
                        Ok(serde_json::Value::Null)
                    } else {
                        Err(anyhow::anyhow!("uuid mismatch"))
                    }
                } else {
                    Err(anyhow::anyhow!("peer connection is not initialized"))
                }
            }
            RequestInner::LogInfo(message) => {
                info!("webview: {}", message);
                Ok(serde_json::Value::Null)
            }
            RequestInner::LogWarn(message) => {
                warn!("webview: {}", message);
                Ok(serde_json::Value::Null)
            }
            RequestInner::LogError(message) => {
                error!("webview: {}", message);
                Ok(serde_json::Value::Null)
            }
        }
    }

    pub async fn terminate(self) -> Result<()> {
        if let Err(_) = self.manager_sender.send(ManagerMessage::Stop) {
            error!("failed to send stop signal");
        }

        self.manager.await?;

        Ok(())
    }
}

/// Voicevox VSTのエディタの設定ファイルのパスを返す
pub fn editor_config_path() -> std::path::PathBuf {
    common::data_dir().join("config.json")
}

/// Voicevox本家のconfig.jsonのパスを返す
pub fn original_config_path() -> std::path::PathBuf {
    // Windows: %APPDATA%/voicevox/config.json
    // macOS: ~/Library/Application Support/voicevox/config.json
    // Linux: ~/.config/voicevox/config.json
    if cfg!(target_os = "windows") {
        let appdata = std::env::var("APPDATA").unwrap();
        std::path::PathBuf::from(appdata).join("voicevox/config.json")
    } else if cfg!(target_os = "macos") {
        let home = std::env::var("HOME").unwrap();
        std::path::PathBuf::from(home).join("Library/Application Support/voicevox/config.json")
    } else {
        let home = std::env::var("HOME").unwrap();
        std::path::PathBuf::from(home).join(".config/voicevox/config.json")
    }
}
