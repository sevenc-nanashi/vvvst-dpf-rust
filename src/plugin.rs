use crate::{
    common,
    model::{ChannelMode, Phrase, Routing, SingingVoiceKey, Track, TrackId},
    saturating_ext::SaturatingMath,
    ui::UiNotification,
    vst_common::RUNTIME,
};
use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as base64, Engine as _};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::Write as _,
    sync::{Arc, Once},
};
use tokio::sync::{mpsc::UnboundedSender, RwLock};
use tracing::{info, instrument};

pub struct PluginImpl {
    pub notification_sender: Option<UnboundedSender<UiNotification>>,

    pub params: Arc<RwLock<PluginParams>>,
    pub critical_params: Arc<RwLock<CriticalPluginParams>>,
    pub mix: Arc<RwLock<Mixes>>,

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

/// 再生に不要なパラメータ。
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct PluginParams {
    pub project: Option<String>,
    pub phrases: Vec<Phrase>,

    pub voices: HashMap<SingingVoiceKey, serde_bytes::ByteBuf>,
}

/// 再生時に必要なパラメータ。可能な限りwriteロックを取る時間は短くすること。
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct CriticalPluginParams {
    pub tracks: HashMap<TrackId, Track>,
    pub routing: Routing,
}

/// VSTに保存する用のパラメータ。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub params: serde_bytes::ByteBuf,
    pub critical_params: serde_bytes::ByteBuf,
}

static INIT: Once = Once::new();

impl PluginImpl {
    pub fn new(params: PluginParams, critical_params: CriticalPluginParams) -> Self {
        INIT.call_once(|| {
            let log_dir = common::log_dir();
            if !log_dir.exists() {
                if fs_err::create_dir_all(&log_dir).is_err() {
                    return;
                }
            }
            let dest = log_dir.join(format!(
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
                .with_max_level(if cfg!(debug_assertions) {
                    tracing::Level::DEBUG
                } else {
                    tracing::Level::INFO
                })
                .with_writer(writer)
                .with_ansi(false)
                .try_init();
        });
        PluginImpl {
            notification_sender: None,
            params: Arc::new(RwLock::new(params)),
            critical_params: Arc::new(RwLock::new(critical_params)),
            mix: Arc::new(RwLock::new(Mixes::default())),

            prev_position: 0,
            prev_is_playing: false,

            current_position: 0.0,
            current_position_updated: false,
        }
    }

    #[instrument(skip(this_ref))]
    pub async fn update_audio_samples(
        this_ref: Arc<RwLock<PluginImpl>>,
        new_sample_rate: Option<f32>,
    ) {
        let (mix, params, critical_params) = {
            let this_ref = this_ref.read().await;
            (
                Arc::clone(&this_ref.mix),
                Arc::clone(&this_ref.params),
                Arc::clone(&this_ref.critical_params),
            )
        };
        let new_sample_rate = {
            let mix = mix.read().await;

            new_sample_rate.unwrap_or(mix.sample_rate)
        };
        if new_sample_rate == 0.0 {
            info!("sample rate is 0, refusing to update mixes");
            return;
        }

        let params = params.read().await;
        let phrases = &params.phrases;
        let voices = &params.voices;

        info!("updating mixes using {} phrases", phrases.len());

        let max_start = phrases
            .iter()
            .map(|phrase| phrase.start)
            .fold(0.0, f32::max);
        let mut new_samples = HashMap::new();
        let mut samples_len = (max_start * new_sample_rate) as usize;
        let critical_params = critical_params.read().await;
        for track_id in critical_params.tracks.keys() {
            new_samples.insert(track_id.clone(), vec![0.0; samples_len]);
        }
        drop(critical_params);

        for phrase in phrases {
            if let Some(voice) = phrase.voice.as_ref().and_then(|v| voices.get(v)) {
                let Some(new_samples) = new_samples.get_mut(&phrase.track_id) else {
                    continue;
                };
                let mut wav = wav_io::reader::Reader::from_vec(voice.to_vec()).unwrap();
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
            } else {
                for note in phrase.notes.iter() {
                    let start = (note.start * new_sample_rate).floor().max(0.0) as usize;
                    let end = (note.end * new_sample_rate).floor() as usize;
                    let mut synth =
                        crate::synthesizer::SynthVoice::new(new_sample_rate, note.note_number);

                    if let Some(new_samples) = new_samples.get_mut(&phrase.track_id) {
                        let padded_end =
                            end + (new_sample_rate * (synth.amplifier.release + 0.1)) as usize + 1;
                        if padded_end > new_samples.len() {
                            new_samples.resize(padded_end, 0.0);
                            if padded_end > samples_len {
                                samples_len = padded_end;
                            }
                        }
                        let mut frame = start;
                        while let Some(sample) = synth.process() {
                            new_samples[frame] = new_samples[frame].saturating_add(sample as f32);
                            frame += 1;
                            if frame == end {
                                synth.note_off();
                            }
                        }
                    }
                }
            }
        }

        info!(
            "mixes updated, {} tracks, {} samples",
            new_samples.len(),
            samples_len
        );

        let mut mix = mix.write().await;
        mix.samples = new_samples;
        mix.sample_rate = new_sample_rate;
        mix.samples_len = samples_len;
    }

    // NOTE: DPFはバイナリ文字列を扱えないので、base64エンコードを挟む
    pub fn set_state(&self, state_base64: &str) -> Result<()> {
        if state_base64.is_empty() {
            return Ok(());
        }
        let state_compressed = base64.decode(state_base64)?;
        let state = zstd::decode_all(state_compressed.as_slice())?;
        let state: State = bincode::deserialize(&state)?;
        let mut params = self.params.blocking_write();
        let mut critical_params = self.critical_params.blocking_write();
        *params = bincode::deserialize(&state.params)?;
        *critical_params = bincode::deserialize(&state.critical_params)?;

        Ok(())
    }

    pub fn get_state(&self) -> Result<String> {
        let params = self.params.blocking_read();
        let critical_params = self.critical_params.blocking_read();
        let state = bincode::serialize(&State {
            params: bincode::serialize(&*params)?.into(),
            critical_params: bincode::serialize(&*critical_params)?.into(),
        })?;
        // 4以降は時間がかかるわりにそれほど効果が無いので3で固定する
        let state_compressed = zstd::encode_all(state.as_slice(), 3).unwrap();
        Ok(base64.encode(state_compressed.as_slice()))
    }

    pub fn run(
        this_ref: Arc<RwLock<PluginImpl>>,
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
        if let Ok(this) = this_ref.try_read() {
            if let (Ok(mix), Ok(critical_params)) =
                (this.mix.try_read(), this.critical_params.try_read())
            {
                if mix.sample_rate != sample_rate {
                    let this_ref = Arc::clone(&this_ref);
                    RUNTIME.spawn(async move {
                        PluginImpl::update_audio_samples(this_ref, Some(sample_rate)).await;
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
                            let solo_track_exists =
                                critical_params.tracks.iter().any(|(_, track)| track.solo);
                            for (track_id, track) in critical_params.tracks.iter() {
                                if solo_track_exists {
                                    if !track.solo {
                                        continue;
                                    }
                                } else if track.mute {
                                    continue;
                                }
                                let Some(track_samples) = &samples.get(track_id) else {
                                    continue;
                                };

                                let Some(&channel_index) =
                                    critical_params.routing.channel_index.get(track_id)
                                else {
                                    continue;
                                };
                                let channel_index = channel_index as usize;
                                match critical_params.routing.channel_mode {
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

            if (this.prev_position, this.prev_is_playing) != (current_sample, is_playing) {
                drop(this);
                if let Ok(mut this) = this_ref.try_write() {
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
    }
}
