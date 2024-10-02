use crate::model::{Phrase, SingingVoiceKey};
use crate::ui::PluginUiImpl;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak, mpsc::{Receiver, Sender}},
};

pub struct PluginImpl {
    pub receiver: Option<Receiver<ToPluginMessage>>,
}

impl PluginImpl {
    pub fn new() -> Self {
        PluginImpl {
            receiver: None,
        }
    }
}

pub enum ToPluginMessage {
    SetPhrases(Vec<Phrase>),
    SetVoices(HashMap<SingingVoiceKey, String>),
}
