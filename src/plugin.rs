use crate::model::{Phrase, SingingVoiceKey};
use crate::ui::{PluginUiImpl, UiNotification};
use base64::{engine::general_purpose::STANDARD as base64, Engine as _};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::Write as _,
    sync::{Arc, Mutex, Once, Weak},
};
use tokio::sync::{mpsc::UnboundedSender, RwLock};

pub struct PluginImpl {
    pub notification_sender: Option<UnboundedSender<UiNotification>>,

    pub params: Arc<RwLock<PluginParams>>,
    pub mixes: Arc<RwLock<Vec<f32>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginParams {
    pub project: Option<String>,
    pub phrases: Vec<Phrase>,
    pub voices: HashMap<SingingVoiceKey, Vec<u8>>,
}

static INIT: Once = Once::new();

impl PluginImpl {
    pub fn new(params: PluginParams) -> Self {
        INIT.call_once(|| {
            if option_env!("VVVST_LOG").map_or(false, |v| v.len() > 0) {
                let dest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR").to_string())
                    .join("logs")
                    .join(format!(
                        "{}.log",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs()
                    ));

                let Ok(writer) = std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&dest)
                else {
                    return;
                };

                let default_panic_hook = std::panic::take_hook();

                std::panic::set_hook(Box::new(move |info| {
                    let mut panic_writer =
                        std::fs::File::create(dest.with_extension("panic")).unwrap();
                    let _ = writeln!(panic_writer, "{:?}", info);

                    default_panic_hook(info);
                }));

                let _ = tracing_subscriber::fmt()
                    .with_writer(writer)
                    .with_ansi(false)
                    .try_init();
            }

            // TODO: ちゃんとエラーダイアログを出す
            let default_panic_hook = std::panic::take_hook();

            std::panic::set_hook(Box::new(move |info| {
                rfd::MessageDialog::new()
                    .set_title("VVVST: Panic")
                    .set_description(&format!("VVVST Panicked: {:?}", info))
                    .set_level(rfd::MessageLevel::Error)
                    .set_buttons(rfd::MessageButtons::Ok)
                    .show();

                default_panic_hook(info);

                std::process::exit(1);
            }));
        });
        PluginImpl {
            notification_sender: None,
            params: Arc::new(RwLock::new(params)),
            mixes: Arc::new(RwLock::new(vec![])),
        }
    }

    pub async fn update_audio_samples(&self) {}

    // メモ：DPFはバイナリ文字列を扱えないので、base64エンコードを挟む
    pub async fn set_state(&self, state_base64: &str) {
        if state_base64.is_empty() {
            return;
        }
        let mut params = self.params.write().await;
        let state = base64.decode(state_base64).unwrap();
        *params = bincode::deserialize(&state).unwrap();
    }

    pub async fn get_state(&self) -> String {
        let params = self.params.read().await;
        base64.encode(&bincode::serialize(&*params).unwrap())
    }
}
