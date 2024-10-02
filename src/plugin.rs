use crate::model::{Phrase, SingingVoiceKey};
use crate::ui::PluginUiImpl;
use std::{
    collections::HashMap,
    io::Write as _,
    sync::{
        mpsc::{Receiver, Sender},
        Arc, Mutex, Once, Weak,
    },
};

pub struct PluginImpl {
    pub receiver: Option<Receiver<ToPluginMessage>>,
}

static INIT: Once = Once::new();

impl PluginImpl {
    pub fn new() -> Self {
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
        PluginImpl { receiver: None }
    }
}

pub enum ToPluginMessage {
    SetPhrases(Vec<Phrase>),
    SetVoices(HashMap<SingingVoiceKey, String>),
}
