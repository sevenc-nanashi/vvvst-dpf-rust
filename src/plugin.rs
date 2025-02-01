use crate::{
    common,
    ipc_model::ChannelMode,
    saturating_ext::SaturatingMath,
    state::{deserialize_state, serialize_state, CriticalPluginParams, Mixes, PluginParams},
    ui::UiNotification,
    vst_common::RUNTIME,
};
use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as base64, Engine as _};
use itertools::{izip, Itertools};
use ordered_float::OrderedFloat;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    io::Write as _,
    sync::{Arc, Once},
};
use tokio::sync::{mpsc::UnboundedSender, Mutex, RwLock};
use tracing::{debug, info, instrument};

pub struct PluginImpl {
    pub notification_sender: Option<UnboundedSender<UiNotification>>,

    pub rtc_sample_rate: Option<f32>,
    pub rtc_samples: Option<tokio::sync::mpsc::UnboundedReceiver<Vec<(f32, f32)>>>,
    pub rtc_samples_buffer: VecDeque<(f32, f32)>,

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

            rtc_sample_rate: None,
            rtc_samples: None,
            rtc_samples_buffer: VecDeque::new(),

            prev_position: 0,
            prev_is_playing: false,

            current_position: 0.0,
            current_position_updated: false,
        }
    }

    #[instrument(skip(this_ref))]
    pub async fn update_audio_samples(
        this_ref: Arc<Mutex<PluginImpl>>,
        new_sample_rate: Option<f32>,
    ) {
        let (mix, params, critical_params) = {
            let this_ref = this_ref.lock().await;
            (
                Arc::clone(&this_ref.mix),
                Arc::clone(&this_ref.params),
                Arc::clone(&this_ref.critical_params),
            )
        };
        let (sample_rate, sample_rate_changed) = {
            let mix = mix.read().await;

            (
                new_sample_rate.unwrap_or(mix.sample_rate),
                new_sample_rate != Some(mix.sample_rate),
            )
        };
        if sample_rate == 0.0 {
            info!("sample rate is 0, refusing to update mixes");
            return;
        }

        let params = params.read().await;
        let phrases = &params.phrases;
        let voices = &params.voices;

        let (mix_source, mix_samples_len) = {
            let mix = mix.read().await;
            (mix.source.clone(), mix.samples_len)
        };

        let max_start = phrases
            .iter()
            .map(|phrase| phrase.start)
            .fold(0.0.into(), OrderedFloat::<f32>::max);
        let mut new_samples = HashMap::new();
        let mut samples_len = ((max_start * sample_rate).0 as usize).max(mix_samples_len);

        let track_ids = {
            critical_params
                .read()
                .await
                .tracks
                .keys()
                .cloned()
                .collect::<HashSet<_>>()
        };

        let added_phrases = phrases
            .iter()
            .filter(|phrase| !mix_source.contains(phrase))
            .collect::<HashSet<_>>();
        let removed_phrases = mix_source
            .iter()
            .filter(|phrase| !phrases.contains(phrase))
            .collect::<HashSet<_>>();

        if added_phrases.is_empty() && removed_phrases.is_empty() {
            if sample_rate_changed {
                let mut mix = mix.write().await;
                mix.sample_rate = sample_rate;
            }
            debug!("no phrases added or removed, skipping mix update");
            return;
        }

        info!(
            "updating mixes using {} phrases ({} added, {} removed)",
            phrases.len(),
            added_phrases.len(),
            removed_phrases.len()
        );

        static FRAMES_PER_SECTION: usize = 32768;

        let mut updated_sections = track_ids
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    vec![false; samples_len / FRAMES_PER_SECTION + 1],
                )
            })
            .collect::<HashMap<_, _>>();
        for phrase in added_phrases.iter().chain(removed_phrases.iter()) {
            let updated_sections = updated_sections.entry(phrase.track_id.clone()).or_default();
            let start = (phrase.start * sample_rate).floor() as usize;

            let end = start + (phrase.duration(voices) * sample_rate as f32) as usize;
            let start_section = start / FRAMES_PER_SECTION;
            let end_section = end / FRAMES_PER_SECTION;
            if samples_len < end {
                samples_len = end;
            }
            if end_section >= updated_sections.len() {
                updated_sections.resize(end_section + 1, false);
            }
            for section in start_section..=end_section {
                updated_sections[section] = true;
            }
        }

        for track_id in track_ids {
            new_samples.insert(track_id.clone(), vec![0.0; samples_len]);
        }

        let mut computed_phrases = 0;
        for phrase in phrases {
            let start = (phrase.start * sample_rate).floor() as usize;
            let end = start + (phrase.duration(voices) * sample_rate as f32) as usize;
            let start_section = start / FRAMES_PER_SECTION;
            let end_section = end / FRAMES_PER_SECTION;
            let mut updated = false;
            for section in start_section..=end_section {
                if updated_sections[&phrase.track_id][section] {
                    updated = true;
                    break;
                }
            }
            if !updated {
                continue;
            }
            computed_phrases += 1;
            if let Some(voice) = phrase.voice.as_ref().and_then(|v| voices.get(v)) {
                let Some(new_samples) = new_samples.get_mut(&phrase.track_id) else {
                    continue;
                };
                let mut wav = voice.reader();
                let header = wav.read_header().unwrap();
                let base_samples = wav.get_samples_f32().unwrap();
                let samples = if header.channels == 1 {
                    base_samples
                } else {
                    wav_io::utils::stereo_to_mono(base_samples)
                };
                let samples =
                    wav_io::resample::linear(samples, 1, header.sample_rate, (sample_rate) as u32);
                let start = (phrase.start * sample_rate).floor() as isize;
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
                    let start = (note.start * sample_rate).floor().max(0.0) as usize;
                    let end = (note.end * sample_rate).floor() as usize;
                    let mut synth =
                        crate::synthesizer::SynthVoice::new(sample_rate, note.note_number);

                    if let Some(new_samples) = new_samples.get_mut(&phrase.track_id) {
                        let padded_end =
                            end + (sample_rate * (crate::synthesizer::RELEASE + 0.1)) as usize + 1;
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

        let num_updated_sections = updated_sections
            .iter()
            .map(|(_, v)| v.iter().filter(|&&b| b).count())
            .sum::<usize>();

        let mut mix = mix.write().await;
        let mut copies = 0;
        for (track_id, updated_sections) in updated_sections {
            let mix_samples = mix
                .samples
                .entry(track_id.clone())
                .or_insert_with(|| vec![0.0; samples_len]);
            if mix_samples.len() < samples_len {
                mix_samples.resize(samples_len, 0.0);
            }
            let mut current_frame = 0;
            for (sections, is_updated) in updated_sections.iter().dedup_with_count() {
                let start = current_frame;
                current_frame += sections * FRAMES_PER_SECTION;
                let end = current_frame;
                if !is_updated {
                    continue;
                }
                let end = end.min(mix_samples.len() - 1);
                if start >= end {
                    continue;
                }
                copies += 1;
                let new_samples = new_samples.get(&track_id).unwrap();
                mix_samples[start..end].copy_from_slice(&new_samples[start..end]);
            }
        }
        mix.sample_rate = sample_rate;
        mix.samples_len = samples_len;
        mix.source = phrases.clone();
        drop(mix);

        info!(
            "mixes updated, {} sections updated using {} phrases, {} copies, {} frames",
            num_updated_sections,
            computed_phrases,
            copies,
            FRAMES_PER_SECTION * num_updated_sections
        );
    }

    // NOTE: DPFはバイナリ文字列を扱えないので、base64エンコードを挟む
    pub fn set_state(&self, state_base64: &str) -> Result<()> {
        if state_base64.is_empty() {
            return Ok(());
        }
        let state_compressed = base64.decode(state_base64)?;
        let (state_params, state_critical_params) = deserialize_state(&state_compressed)?;
        let mut params = self.params.blocking_write();
        let mut critical_params = self.critical_params.blocking_write();
        *params = state_params;
        *critical_params = state_critical_params;

        Ok(())
    }

    pub fn get_state(&self) -> Result<String> {
        let params = self.params.blocking_read();
        let critical_params = self.critical_params.blocking_read();
        let state = serialize_state(&params, &critical_params)?;
        drop(params);
        drop(critical_params);
        Ok(base64.encode(state.as_slice()))
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
            if let (Ok(mix), Ok(critical_params)) =
                (this.mix.try_read(), this.critical_params.try_read())
            {
                this.write_mix(
                    &this_ref,
                    &mix,
                    &critical_params,
                    outputs,
                    sample_rate,
                    is_playing,
                    current_sample,
                );
            }
            this.update_playing_state(is_playing, current_sample, sample_rate);
            this.write_rtc_samples(outputs);
        }
    }

    fn write_mix(
        &self,
        this_ref: &Arc<Mutex<PluginImpl>>,
        mix: &Mixes,
        critical_params: &CriticalPluginParams,
        outputs: &mut [&mut [f32]],
        sample_rate: f32,
        is_playing: bool,
        current_sample: i64,
    ) {
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
                                    .saturating_add(track_samples[current_frame] * track.gain);
                            }
                            ChannelMode::Stereo => {
                                let (left_multiplier, right_multiplier) = if track.pan < 0.0 {
                                    (1.0, 1.0 + track.pan)
                                } else {
                                    (1.0 - track.pan, 1.0)
                                };
                                outputs[channel_index * 2][i] = outputs[channel_index * 2][i]
                                    .saturating_add(
                                        track_samples[current_frame] * track.gain * left_multiplier,
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

    fn update_playing_state(&mut self, is_playing: bool, current_sample: i64, sample_rate: f32) {
        if self.prev_is_playing != is_playing {
            self.prev_is_playing = is_playing;
            if let Some(sender) = &self.notification_sender {
                if sender
                    .send(UiNotification::UpdatePlayingState(is_playing))
                    .is_err()
                {
                    self.notification_sender = None;
                }
            }
        }
        if self.prev_position != current_sample {
            self.prev_position = current_sample;
            self.current_position = (current_sample as f32 / sample_rate).max(0.0);
            self.current_position_updated = true;
        }
    }

    fn write_rtc_samples(&mut self, outputs: &mut [&mut [f32]]) {
        if let (Some(rtc_sample_rate), Some(samples_receiver)) =
            (self.rtc_sample_rate, self.rtc_samples.as_mut())
        {
            let mut buffer = VecDeque::with_capacity(outputs[0].len());
            while let Ok(samples) = samples_receiver.try_recv() {
                buffer.extend(samples);
            }
            self.rtc_samples_buffer.extend(buffer);
            let buffer_seconds = outputs[0].len() as f32 / rtc_sample_rate;
            let frames = (buffer_seconds * rtc_sample_rate) as usize;
            let frames = frames.min(self.rtc_samples_buffer.len());
            let samples = self.rtc_samples_buffer.drain(..frames).collect::<Vec<_>>();
            for (output_l, &(sample_l, _)) in izip!(outputs[0].iter_mut(), &samples) {
                *output_l = output_l.saturating_add(sample_l);
            }
            for (output_r, &(_, sample_r)) in izip!(outputs[1].iter_mut(), &samples) {
                *output_r = output_r.saturating_add(sample_r);
            }
        }
    }
}
