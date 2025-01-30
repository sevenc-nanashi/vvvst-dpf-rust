use std::f32::consts::PI;

pub struct SquareOscillator {
    two_pi: f32,
    w0: f32,
    phase: f32,
}

impl SquareOscillator {
    fn new(sample_rate: f32, frequency: f32) -> Self {
        let two_pi = 2.0 * PI;
        let w0 = two_pi * frequency / sample_rate;
        Self {
            two_pi,
            w0,
            phase: 0.0,
        }
    }

    fn process(&mut self) -> f32 {
        let mut y = if self.phase < PI { 1.0 } else { -1.0 };

        y += self.poly_blep(0.0);
        y -= self.poly_blep(0.5);

        self.phase += self.w0;
        if self.phase >= self.two_pi {
            self.phase -= self.two_pi;
        }
        return y;
    }

    fn poly_blep(&self, offset: f32) -> f32 {
        let dt = self.w0 / self.two_pi;
        let mut t = self.phase / self.two_pi;
        t += offset;
        if t >= 1.0 {
            t -= 1.0;
        }

        if t <= dt {
            let a = t / dt;
            return a + a - a * a - 1.0;
        } else if t >= 1.0 - dt {
            let a = (t - 1.0) / dt;
            return a * a + a + a + 1.0;
        } else {
            return 0.0;
        }
    }
}

pub struct LowPassFilter {
    a1: f32,
    a2: f32,
    b0: f32,
    b1: f32,
    b2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl LowPassFilter {
    fn new(sample_rate: f32, cutoff: f32, q: f32) -> Self {
        let w0 = 2.0 * PI * cutoff / sample_rate;
        let alpha = w0.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;

        let a1 = -2.0 * w0.cos() / a0;
        let a2 = (1.0 - alpha) / a0;
        let b0 = (1.0 - w0.cos()) / 2.0 / a0;
        let b1 = (1.0 - w0.cos()) / a0;
        let b2 = (1.0 - w0.cos()) / 2.0 / a0;

        Self {
            a1,
            a2,
            b0,
            b1,
            b2,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let y = self.b0 * input + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;

        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = y;

        return y;
    }
}

pub struct Amplifier {
    dt: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    t: f32,
    gain: f32,
    state: State,
    gain_at_note_off_start: f32,
    time_at_note_off_start: f32,
}

enum State {
    NoteOn,
    NoteOff,
}

impl Amplifier {
    fn new(sample_rate: f32, attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        if attack < 0.001 || decay < 0.001 || sustain < 0.0 || sustain > 1.0 || release < 0.001 {
            panic!("Invalid ADSR parameters.");
        }
        let dt = 1.0 / sample_rate;
        Self {
            dt,
            attack,
            decay,
            sustain,
            release,
            t: 0.0,
            gain: 0.0,
            state: State::NoteOn,
            gain_at_note_off_start: 0.0,
            time_at_note_off_start: 0.0,
        }
    }

    fn note_off(&mut self) {
        self.state = State::NoteOff;
        self.time_at_note_off_start = self.t;
        self.gain_at_note_off_start = self.gain;
    }

    fn process(&mut self, input: f32) -> f32 {
        match self.state {
            State::NoteOn => {
                if self.t < self.attack {
                    self.gain = self.t / self.attack;
                } else {
                    self.gain =
                        self.exponential_decay(1.0, self.sustain, self.t - self.attack, self.decay);
                }
            }
            State::NoteOff => {
                self.gain = self.exponential_decay(
                    self.gain_at_note_off_start,
                    0.0,
                    self.t - self.time_at_note_off_start,
                    self.release,
                );
            }
        }

        self.t += self.dt;

        return input * self.gain;
    }

    fn exponential_decay(
        &self,
        start_value: f32,
        end_value: f32,
        time: f32,
        time_constant: f32,
    ) -> f32 {
        let max_time = time_constant * 7.0;
        let exp_factor = if time < max_time {
            (-time / time_constant).exp()
        } else {
            0.0
        };
        return end_value + (start_value - end_value) * exp_factor;
    }
}

pub struct SynthVoice {
    pub oscillator: SquareOscillator,
    pub low_pass_filter: LowPassFilter,
    pub amplifier: Amplifier,
    pub frames: usize,
    pub end_frame: Option<usize>,
    pub sample_rate: f32,
    pub volume: f32,
}

impl SynthVoice {
    pub fn new(sample_rate: f32, note_number: u8) -> Self {
        let cutoff = 2500.0;
        let q = 1.0 / 2.0_f32.sqrt();
        let key_track = 0.25;
        let attack = 0.001;
        let decay = 0.18;
        let sustain = 0.5;
        let release = 0.02;
        let volume = 0.1;

        let frequency = 440.0 * 2.0_f32.powf((note_number as f32 - 69.0) / 12.0);
        let filter_freq =
            cutoff * 2.0_f32.powf((((note_number as i8) - 60) as f32 * key_track) / 12.0);

        Self {
            oscillator: SquareOscillator::new(sample_rate, frequency),
            low_pass_filter: LowPassFilter::new(sample_rate, filter_freq, q),
            amplifier: Amplifier::new(sample_rate, attack, decay, sustain, release),
            frames: 0,
            end_frame: None,
            sample_rate,
            volume,
        }
    }

    pub fn process(&mut self) -> Option<f32> {
        if let Some(end_frame) = self.end_frame {
            if self.frames >= end_frame {
                return None;
            }
        }
        let mut y = self.oscillator.process();
        y = self.low_pass_filter.process(y);
        y = self.amplifier.process(y);
        self.frames += 1;
        return Some(y * self.volume);
    }

    pub fn note_off(&mut self) {
        self.amplifier.note_off();
        self.end_frame = Some(self.frames + (self.sample_rate * self.amplifier.release) as usize);
    }
}
