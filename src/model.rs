use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RequestId(pub u32);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub request_id: RequestId,
    pub payload: Result<Value, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub request_id: RequestId,
    pub inner: RequestInner,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "payload")]
pub enum RequestInner {
    GetVersion,
    GetProjectName,

    GetConfig,

    GetProject,
    SetProject(String),

    SetPhrases(Vec<Phrase>),

    GetVoices,
    SetVoices(HashMap<SingingVoiceKey, String>),

    SetTracks(HashMap<TrackId, Track>),

    SetRouting(Routing),
    GetRouting,

    ShowImportFileDialog(ShowImportFileDialog),

    ReadFile(String),

    ExportProject,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SingingVoiceKey(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TrackId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowImportFileDialog {
    pub title: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub filters: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Phrase {
    pub start: f32,
    pub track_id: TrackId,
    pub voice: SingingVoiceKey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetPhraseResult {
    pub missing_voices: Vec<SingingVoiceKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub name: String,

    pub solo: bool,
    pub mute: bool,
    pub pan: f32,
    pub gain: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Routing {
    pub channel_mode: ChannelMode,
    pub channel_index: HashMap<TrackId, u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ChannelMode {
    Mono,
    #[default]
    Stereo,
}
