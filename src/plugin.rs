use crate::common::RUNTIME;
use crate::model::{Phrase, SingingVoiceKey};
use crate::ui::UiNotification;
use base64::{engine::general_purpose::STANDARD as base64, Engine as _};
use serde::{
    de::{MapAccess, Visitor},
    ser::SerializeMap,
    Deserialize, Deserializer, Serialize, Serializer,
};
use std::{
    collections::HashMap,
    io::Write as _,
    sync::{Arc, Once},
};
use tokio::sync::{mpsc::UnboundedSender, Mutex, RwLock};
use tracing::{info, instrument};

pub struct PluginImpl {
    pub notification_sender: Option<UnboundedSender<UiNotification>>,

    pub params: Arc<RwLock<PluginParams>>,
    pub mix: Arc<RwLock<Mixes>>,

    prev_position: usize,
    prev_is_playing: bool,
}
impl std::fmt::Debug for PluginImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginImpl").finish()
    }
}

pub struct Mixes {
    pub samples: Vec<f32>,
    pub sample_rate: f32,
}
impl Default for Mixes {
    fn default() -> Self {
        Mixes {
            samples: Vec::new(),
            sample_rate: 44100.0,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct PluginParams {
    pub project: Option<String>,
    pub phrases: Vec<Phrase>,

    #[serde(
        serialize_with = "serialize_voices",
        deserialize_with = "deserialize_voices"
    )]
    pub voices: HashMap<SingingVoiceKey, Vec<u8>>,
}

// https://github.com/serde-rs/serde/issues/2554#issuecomment-1666887206
fn serialize_voices<S>(
    voices: &HashMap<SingingVoiceKey, Vec<u8>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut map = serializer.serialize_map(Some(voices.len()))?;
    for (key, bytes) in voices {
        let value = serde_bytes::ByteBuf::from(bytes.to_owned());
        map.serialize_entry(&key.0, &value)?;
    }
    map.end()
}

fn deserialize_voices<'de, D>(
    deserializer: D,
) -> Result<HashMap<SingingVoiceKey, Vec<u8>>, D::Error>
where
    D: Deserializer<'de>,
{
    struct VoicesVisitor;

    impl<'de> Visitor<'de> for VoicesVisitor {
        type Value = HashMap<SingingVoiceKey, Vec<u8>>;
        fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
        where
            M: MapAccess<'de>,
        {
            let mut generic_tags = HashMap::new();
            while let Some(key) = map.next_key::<String>()? {
                let value = map.next_value::<serde_bytes::ByteBuf>()?;
                generic_tags.insert(SingingVoiceKey(key), value.into_vec());
            }
            Ok(generic_tags)
        }

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a map")
        }
    }

    deserializer.deserialize_map(VoicesVisitor)
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
                    let backtrace = std::backtrace::Backtrace::force_capture();
                    let _ = writeln!(panic_writer, "{}\n{}", info, backtrace);

                    default_panic_hook(info);
                }));

                let _ = tracing_subscriber::fmt()
                    .with_writer(writer)
                    .with_ansi(false)
                    .try_init();
            }
        });
        PluginImpl {
            notification_sender: None,
            params: Arc::new(RwLock::new(params)),
            mix: Arc::new(RwLock::new(Mixes::default())),

            prev_position: 0,
            prev_is_playing: false,
        }
    }

    #[instrument]
    pub async fn update_audio_samples(&self, new_sample_rate: Option<f32>) {
        let params = self.params.read().await;
        let phrases = &params.phrases;
        let voices = &params.voices;
        let mut mix = self.mix.write().await;
        mix.samples.clear();
        info!("updating mixes using {} phrases", phrases.len());

        let new_sample_rate = new_sample_rate.unwrap_or(mix.sample_rate);

        let max_start = phrases
            .iter()
            .map(|phrase| phrase.start)
            .fold(0.0, f32::max);
        let mut new_samples = vec![0.0; (max_start * new_sample_rate) as usize];
        for phrase in phrases {
            if let Some(voice) = voices.get(&phrase.voice) {
                let mut wav = wav_io::reader::Reader::from_vec(voice.clone()).unwrap();
                let header = wav.read_header().unwrap();
                let base_samples = wav.get_samples_f32().unwrap();
                let samples = if header.channels == 1 {
                    base_samples
                } else {
                    wav_io::utils::stereo_to_mono(base_samples)
                };
                let samples = wav_io::resample::linear(
                    samples,
                    1,
                    header.sample_rate,
                    (new_sample_rate) as u32,
                );
                let start = (phrase.start * new_sample_rate).floor() as isize;
                let end = start + samples.len() as isize;

                if end > new_samples.len() as isize {
                    new_samples.resize(end as usize, 0.0);
                }
                for i in 0..samples.len() {
                    let frame = start + i as isize;
                    if frame < 0 {
                        continue;
                    }
                    let frame = frame as usize;
                    if new_samples[frame] > f32::MAX - samples[i] {
                        new_samples[frame] = f32::MAX;
                    } else if new_samples[frame] < f32::MIN - samples[i] {
                        new_samples[frame] = f32::MIN;
                    } else {
                        new_samples[frame] += samples[i];
                    }
                }
            }
        }

        info!("mixes updated, {} samples", new_samples.len());

        mix.samples = new_samples;
        mix.sample_rate = new_sample_rate;
    }

    // メモ：DPFはバイナリ文字列を扱えないので、base64エンコードを挟む
    pub async fn set_state(&self, state_base64: &str) {
        if state_base64.is_empty() {
            return;
        }
        let mut params = self.params.write().await;
        let state = base64.decode(state_base64).unwrap();
        if let Ok(loaded_params) = bincode::deserialize(&state) {
            *params = loaded_params;
        }
    }

    pub async fn get_state(&self) -> String {
        let params = self.params.read().await;
        base64.encode(&bincode::serialize(&*params).unwrap())
    }

    pub fn run(
        this_ref: Arc<Mutex<PluginImpl>>,
        outputs: &mut [&mut [f32]],
        sample_rate: f32,
        is_playing: bool,
        current_sample: usize,
    ) {
        if let Ok(mut this) = this_ref.try_lock() {
            if let Ok(mix) = this.mix.try_read() {
                let samples = &mix.samples;
                if samples.is_empty() {
                    return;
                }
                if mix.sample_rate != sample_rate {
                    let this_ref = Arc::clone(&this_ref);
                    RUNTIME.spawn(async move {
                        this_ref
                            .lock()
                            .await
                            .update_audio_samples(Some(sample_rate))
                            .await;
                    });
                    return;
                }
                if is_playing {
                    for i in 0..outputs[0].len() {
                        let current_frame = current_sample + i;
                        if current_frame < samples.len() {
                            outputs[0][i] = samples[current_frame];
                            outputs[1][i] = samples[current_frame];
                        } else {
                            outputs[0][i] = 0.0;
                            outputs[1][i] = 0.0;
                        }
                    }
                }
            }

            if this.prev_position.abs_diff(current_sample) > (sample_rate / 10.0) as usize {
                if let Some(sender) = &this.notification_sender {
                    if sender
                        .send(UiNotification::UpdatePosition(
                            (current_sample as f32) / sample_rate,
                        ))
                        .is_err()
                    {
                        this.notification_sender = None;
                    }
                }
            }
            if this.prev_is_playing != is_playing {
                this.prev_is_playing = is_playing;
                if let Some(sender) = &this.notification_sender {
                    if sender
                        .send(UiNotification::UpdatePlayingState(is_playing))
                        .is_err()
                    {
                        this.notification_sender = None;
                    }
                }
            }
        }
    }
}
