use crate::{
    common,
    model::{ChannelMode, Phrase, Routing, SingingVoiceKey, Track, TrackId},
    saturating_ext::SaturatingMath,
    ui::UiNotification,
    vst_common::RUNTIME,
};
use anyhow::Result;
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

    pub voice_caches: HashMap<SingingVoiceKey, Vec<u8>>,

    prev_position: i64,
    prev_is_playing: bool,

    pub current_position: f32,
    pub current_position_updated: bool,
}
impl std::fmt::Debug for PluginImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginImpl").finish()
    }
}

pub struct Mixes {
    pub samples: HashMap<TrackId, Vec<f32>>,
    pub sample_rate: f32,
    pub samples_len: usize,
}
impl Default for Mixes {
    fn default() -> Self {
        Mixes {
            samples: HashMap::new(),
            sample_rate: 0.0,
            samples_len: 0,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct PluginParams {
    pub project: Option<String>,
    pub phrases: Vec<Phrase>,
    pub tracks: HashMap<TrackId, Track>,
    pub routing: Routing,

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
            let dest = common::log_dir().join(format!(
                "{}-plugin.log",
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
                let mut panic_writer = std::fs::File::create(dest.with_extension("panic")).unwrap();
                let backtrace = std::backtrace::Backtrace::force_capture();
                let _ = writeln!(panic_writer, "{}\n{}", info, backtrace);

                default_panic_hook(info);
            }));

            let _ = tracing_subscriber::fmt()
                .with_writer(writer)
                .with_ansi(false)
                .try_init();
        });
        PluginImpl {
            notification_sender: None,
            params: Arc::new(RwLock::new(params)),
            mix: Arc::new(RwLock::new(Mixes::default())),

            voice_caches: HashMap::new(),

            prev_position: 0,
            prev_is_playing: false,

            current_position: 0.0,
            current_position_updated: false,
        }
    }

    #[instrument]
    pub async fn update_audio_samples(&self, new_sample_rate: Option<f32>) {
        let mut mix = self.mix.write().await;
        mix.samples.clear();

        let new_sample_rate = new_sample_rate.unwrap_or(mix.sample_rate);
        if new_sample_rate == 0.0 {
            info!("sample rate is 0, refusing to update mixes");
            return;
        }

        let params = self.params.read().await;
        let phrases = &params.phrases;
        let voices = &params.voices;

        info!("updating mixes using {} phrases", phrases.len());

        let max_start = phrases
            .iter()
            .map(|phrase| phrase.start)
            .fold(0.0, f32::max);
        let mut new_samples = HashMap::new();
        let mut samples_len = (max_start * new_sample_rate) as usize;
        for track_id in params.tracks.keys() {
            new_samples.insert(track_id.clone(), vec![0.0; samples_len]);
        }
        for phrase in phrases {
            if let Some(voice) = voices.get(&phrase.voice) {
                let Some(new_samples) = new_samples.get_mut(&phrase.track_id) else {
                    continue;
                };
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
                    if end as usize > samples_len {
                        samples_len = end as usize;
                    }
                }
                for i in 0..samples.len() {
                    let frame = start + i as isize;
                    if frame < 0 {
                        continue;
                    }
                    let frame = frame as usize;
                    new_samples[frame] = new_samples[frame].saturating_add(samples[i]);
                }
            }
        }

        info!(
            "mixes updated, {} tracks, {} samples",
            new_samples.len(),
            samples_len
        );

        mix.samples = new_samples;
        mix.sample_rate = new_sample_rate;
        mix.samples_len = samples_len;
    }

    // NOTE: DPFはバイナリ文字列を扱えないので、base64エンコードを挟む
    pub fn set_state(&self, state_base64: &str) -> Result<()> {
        if state_base64.is_empty() {
            return Ok(());
        }
        let mut params = self.params.blocking_write();
        let state_compressed = base64.decode(state_base64)?;
        let state = zstd::decode_all(state_compressed.as_slice())?;
        let loaded_params = bincode::deserialize(&state)?;
        *params = loaded_params;

        Ok(())
    }

    pub fn get_state(&self) -> String {
        let params = { self.params.blocking_read().clone() };
        let state = bincode::serialize(&params).unwrap();
        // 22以降は時間がかかるわりにそれほど効果が無いので3で固定する
        let state_compressed = zstd::encode_all(state.as_slice(), 3).unwrap();
        base64.encode(state_compressed.as_slice())
    }

    pub fn run(
        this_ref: Arc<Mutex<PluginImpl>>,
        outputs: &mut [&mut [f32]],
        sample_rate: f32,
        is_playing: bool,
        current_sample: i64,
    ) {
        for output in outputs.iter_mut() {
            for sample in output.iter_mut() {
                *sample = 0.0;
            }
        }
        if let Ok(mut this) = this_ref.try_lock() {
            if let (Ok(mix), Ok(params)) = (this.mix.try_read(), this.params.try_read()) {
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
                let samples = &mix.samples;
                if samples.is_empty() || mix.samples_len == 0 {
                    return;
                }
                if is_playing {
                    for i in 0..outputs[0].len() {
                        let current_frame = current_sample + i as i64;
                        if current_frame < 0 {
                            continue;
                        }
                        let current_frame = current_frame as usize;
                        if current_frame < mix.samples_len {
                            for (track_id, track) in params.tracks.iter() {
                                let Some(track_samples) = &samples.get(track_id) else {
                                    continue;
                                };

                                if current_frame >= track_samples.len() {
                                    continue;
                                }

                                let Some(&channel_index) =
                                    params.routing.channel_index.get(track_id)
                                else {
                                    continue;
                                };
                                let channel_index = channel_index as usize;
                                match params.routing.channel_mode {
                                    ChannelMode::Mono => {
                                        outputs[channel_index][i] = outputs[channel_index][i]
                                            .saturating_add(
                                                track_samples[current_frame] * track.gain,
                                            );
                                    }
                                    ChannelMode::Stereo => {
                                        let (left_multiplier, right_multiplier) = if track.pan < 0.0
                                        {
                                            (1.0, 1.0 + track.pan)
                                        } else {
                                            (1.0 - track.pan, 1.0)
                                        };
                                        outputs[channel_index * 2][i] =
                                            outputs[channel_index * 2][i].saturating_add(
                                                track_samples[current_frame]
                                                    * track.gain
                                                    * left_multiplier,
                                            );
                                        outputs[channel_index * 2 + 1][i] =
                                            outputs[channel_index * 2 + 1][i].saturating_add(
                                                track_samples[current_frame]
                                                    * track.gain
                                                    * right_multiplier,
                                            );
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if this.prev_position != current_sample {
                this.prev_position = current_sample;
                this.current_position = (current_sample as f32 / sample_rate).max(0.0);
                this.current_position_updated = true;
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
