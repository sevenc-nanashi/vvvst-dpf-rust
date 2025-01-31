use crate::{
    ipc_model::{Phrase, Routing, SingingVoiceKey, Track, TrackId},
    voice::Voice,
};
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub struct Mixes {
    pub samples: HashMap<TrackId, Vec<f32>>,
    pub sample_rate: f32,
    pub samples_len: usize,
    pub source: HashSet<Phrase>,
}
impl Default for Mixes {
    fn default() -> Self {
        Mixes {
            samples: HashMap::new(),
            sample_rate: 0.0,
            samples_len: 0,
            source: HashSet::new(),
        }
    }
}

/// 再生に不要なパラメータ。
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct PluginParams {
    pub project: Option<String>,
    pub phrases: HashSet<Phrase>,

    pub voices: HashMap<SingingVoiceKey, Voice>,
}

impl Phrase {
    pub fn duration(&self, voices: &HashMap<SingingVoiceKey, Voice>) -> f32 {
        if let Some(voice) = self.voice.as_ref().and_then(|v| voices.get(v)) {
            voice.duration()
        } else {
            (self
                .notes
                .iter()
                .map(|note| note.end)
                .fold(0.0.into(), OrderedFloat::<f32>::max)
                - self.start)
                .0
        }
    }
}

/// 再生時に必要なパラメータ。可能な限りwriteロックを取る時間は短くすること。
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct CriticalPluginParams {
    pub tracks: HashMap<TrackId, Track>,
    pub routing: Routing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V1State {
    pub params: serde_bytes::ByteBuf,
    pub critical_params: serde_bytes::ByteBuf,
}
