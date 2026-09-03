//! `harmonic_synth` — a polyphonic MIDI instrument plugin (VST3 + CLAP).
//!
//! All synthesis lives in [`harmonic_core`]: band-limited additive tones from
//! the Dirichlet-kernel closed form, one [`PolySynth`] driving a fixed voice
//! array with no allocation on the audio path. This file is only the host glue:
//! parameters, MIDI event routing, and the sample loop.

use harmonic_core::{CharParams, FilterMode, LfoShape, PolySynth};
use nih_plug::prelude::*;
use std::sync::Arc;

/// Voice count. Fixed — the engine never allocates. Unison stacks eat into it.
const MAX_VOICES: usize = 24;

/// Filter response, as a host-visible choice.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
enum FilterKind {
    #[id = "off"]
    #[name = "Off"]
    Off,
    #[id = "lp"]
    #[name = "Low Pass"]
    LowPass,
    #[id = "bp"]
    #[name = "Band Pass"]
    BandPass,
    #[id = "hp"]
    #[name = "High Pass"]
    HighPass,
    #[id = "notch"]
    #[name = "Notch"]
    Notch,
}

impl FilterKind {
    fn to_core(self) -> FilterMode {
        match self {
            FilterKind::Off => FilterMode::Bypass,
            FilterKind::LowPass => FilterMode::Low,
            FilterKind::BandPass => FilterMode::Band,
            FilterKind::HighPass => FilterMode::High,
            FilterKind::Notch => FilterMode::Notch,
        }
    }
}

/// LFO waveform, host-visible.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
enum LfoKind {
    #[id = "sine"]
    Sine,
    #[id = "tri"]
    #[name = "Triangle"]
    Triangle,
    #[id = "saw"]
    Saw,
}

impl LfoKind {
    fn to_core(self) -> LfoShape {
        match self {
            LfoKind::Sine => LfoShape::Sine,
            LfoKind::Triangle => LfoShape::Triangle,
            LfoKind::Saw => LfoShape::Saw,
        }
    }
}

struct HarmonicSynth {
    params: Arc<HarmonicSynthParams>,
    engine: PolySynth<MAX_VOICES>,
    /// Tracks the HQ toggle so we only re-report latency on a real change.
    last_hq: bool,
}

impl HarmonicSynth {
    #[inline]
    fn hq_latency(&self) -> u32 {
        if self.params.hq_mode.value() {
            harmonic_core::Voice::HQ_LATENCY as u32
        } else {
            0
        }
    }
}

#[derive(Params)]
struct HarmonicSynthParams {
    /// 0 → pure fundamental, 1 → bright flat-spectrum pulse. Maps to the
    /// geometric rolloff `r`.
    #[id = "bright"]
    brightness: FloatParam,

    #[id = "attack"]
    attack: FloatParam,

    #[id = "release"]
    release: FloatParam,

    #[id = "gain"]
    gain: FloatParam,

    // --- character ---
    /// Asymmetric saturation into the waveshaper. The "fatten".
    #[id = "drive"]
    drive: FloatParam,

    /// Sine wavefolder depth. Dense evolving upper spectrum.
    #[id = "fold"]
    fold: FloatParam,

    /// Deliberate lo-fi: bit-crush + sample-rate reduction together.
    /// The PPG / DX7 grit, on a knob.
    #[id = "grit"]
    grit: FloatParam,

    // --- FM ---
    /// Phase-modulation depth. 0 = off.
    #[id = "fmamt"]
    fm_amount: FloatParam,

    /// Modulator frequency ÷ fundamental. Integers stay harmonic.
    #[id = "fmratio"]
    fm_ratio: FloatParam,

    /// Operator self-feedback. Sine → saw → noise.
    #[id = "feedbk"]
    feedback: FloatParam,

    // --- filter ---
    #[id = "fltmode"]
    filter_mode: EnumParam<FilterKind>,

    #[id = "fltcut"]
    filter_cutoff: FloatParam,

    #[id = "fltres"]
    filter_res: FloatParam,

    /// Filter envelope → cutoff, in octaves at the envelope peak. Bipolar.
    #[id = "fltenv"]
    filter_env: FloatParam,

    // dedicated filter-envelope ADSR
    #[id = "featk"]
    filter_env_attack: FloatParam,
    #[id = "fedec"]
    filter_env_decay: FloatParam,
    #[id = "fesus"]
    filter_env_sustain: FloatParam,
    #[id = "ferel"]
    filter_env_release: FloatParam,

    // --- voice mode ---
    /// Analog-style free-running oscillator phase (no reset on note-on).
    #[id = "freerun"]
    free_running: BoolParam,

    /// 2× oversample the oscillator + Character stage so drive / fold / grit /
    /// FM don't alias. Adds a few samples of latency; toggling re-syncs the host.
    #[id = "hqmode"]
    hq_mode: BoolParam,

    // --- unison ---
    #[id = "unicnt"]
    unison_voices: IntParam,
    #[id = "unidet"]
    unison_detune: FloatParam,
    #[id = "unispr"]
    unison_spread: FloatParam,

    // --- pitch bend + LFO ---
    #[id = "bendrng"]
    bend_range: IntParam,
    #[id = "lforate"]
    lfo_rate: FloatParam,
    #[id = "lfoshp"]
    lfo_shape: EnumParam<LfoKind>,
    #[id = "lfobrt"]
    lfo_to_bright: FloatParam,
    #[id = "lfovib"]
    lfo_vibrato: FloatParam,
}

impl Default for HarmonicSynth {
    fn default() -> Self {
        Self {
            params: Arc::new(HarmonicSynthParams::default()),
            engine: PolySynth::new(48_000.0),
            last_hq: false,
        }
    }
}

impl Default for HarmonicSynthParams {
    fn default() -> Self {
        Self {
            brightness: FloatParam::new(
                "Brightness",
                0.35,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(25.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),

            attack: FloatParam::new(
                "Attack",
                0.005,
                FloatRange::Skewed {
                    min: 0.0005,
                    max: 2.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" s")
            .with_value_to_string(formatters::v2s_f32_rounded(3)),

            release: FloatParam::new(
                "Release",
                0.18,
                FloatRange::Skewed {
                    min: 0.005,
                    max: 5.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" s")
            .with_value_to_string(formatters::v2s_f32_rounded(3)),

            gain: FloatParam::new(
                "Gain",
                util::db_to_gain(-12.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-60.0),
                    max: util::db_to_gain(0.0),
                    factor: FloatRange::gain_skew_factor(-60.0, 0.0),
                },
            )
            .with_unit(" dB")
            .with_smoother(SmoothingStyle::Linear(15.0))
            .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),

            drive: FloatParam::new("Drive", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),

            fold: FloatParam::new("Fold", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),

            grit: FloatParam::new("Grit", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),

            fm_amount: FloatParam::new(
                "FM Amount",
                0.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 4.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(15.0)),

            fm_ratio: FloatParam::new(
                "FM Ratio",
                1.0,
                FloatRange::Linear { min: 0.5, max: 12.0 },
            )
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            feedback: FloatParam::new(
                "Feedback",
                0.0,
                FloatRange::Linear { min: 0.0, max: 0.9 },
            )
            .with_smoother(SmoothingStyle::Linear(15.0))
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            filter_mode: EnumParam::new("Filter", FilterKind::Off),

            filter_cutoff: FloatParam::new(
                "Cutoff",
                12_000.0,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 20_000.0,
                    factor: FloatRange::skew_factor(-2.5),
                },
            )
            .with_unit(" Hz")
            .with_smoother(SmoothingStyle::Logarithmic(20.0))
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            filter_res: FloatParam::new(
                "Resonance",
                0.15,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),

            filter_env: FloatParam::new(
                "Filter Env",
                0.0,
                FloatRange::Linear { min: -6.0, max: 6.0 },
            )
            .with_unit(" oct")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            filter_env_attack: FloatParam::new(
                "F.Env Attack",
                0.004,
                FloatRange::Skewed {
                    min: 0.0005,
                    max: 2.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" s")
            .with_value_to_string(formatters::v2s_f32_rounded(3)),

            filter_env_decay: FloatParam::new(
                "F.Env Decay",
                0.15,
                FloatRange::Skewed {
                    min: 0.001,
                    max: 4.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" s")
            .with_value_to_string(formatters::v2s_f32_rounded(3)),

            filter_env_sustain: FloatParam::new(
                "F.Env Sustain",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),

            filter_env_release: FloatParam::new(
                "F.Env Release",
                0.25,
                FloatRange::Skewed {
                    min: 0.005,
                    max: 5.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" s")
            .with_value_to_string(formatters::v2s_f32_rounded(3)),

            free_running: BoolParam::new("Free-Run Phase", false),

            hq_mode: BoolParam::new("HQ Mode", false),

            unison_voices: IntParam::new("Unison", 1, IntRange::Linear { min: 1, max: 8 }),

            unison_detune: FloatParam::new(
                "Uni Detune",
                12.0,
                FloatRange::Linear { min: 0.0, max: 50.0 },
            )
            .with_unit(" ct")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            unison_spread: FloatParam::new(
                "Uni Spread",
                0.6,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),

            bend_range: IntParam::new("Bend Range", 2, IntRange::Linear { min: 1, max: 24 })
                .with_unit(" st"),

            lfo_rate: FloatParam::new(
                "LFO Rate",
                5.0,
                FloatRange::Skewed {
                    min: 0.02,
                    max: 30.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            lfo_shape: EnumParam::new("LFO Shape", LfoKind::Sine),

            lfo_to_bright: FloatParam::new(
                "LFO → Bright",
                0.0,
                FloatRange::Linear { min: -1.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(15.0))
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            lfo_vibrato: FloatParam::new(
                "LFO Vibrato",
                0.0,
                FloatRange::Linear { min: 0.0, max: 100.0 },
            )
            .with_unit(" ct")
            .with_smoother(SmoothingStyle::Linear(15.0))
            .with_value_to_string(formatters::v2s_f32_rounded(1)),
        }
    }
}

/// Map the `[0,1]` brightness knob to the geometric rolloff `r ∈ (0,1)`.
/// Quadratic so the low end (where the ear is sensitive to partial count)
/// has more travel.
#[inline]
fn brightness_to_r(b: f32) -> f64 {
    let b = b.clamp(0.0, 1.0) as f64;
    0.02 + (0.9995 - 0.02) * b * b
}

impl Plugin for HarmonicSynth {
    const NAME: &'static str = "Harmonic Synth";
    const VENDOR: &'static str = "harmonic_core";
    const URL: &'static str = "https://example.invalid/harmonic_core";
    const EMAIL: &'static str = "noreply@example.invalid";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn initialize(
        &mut self,
        _layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        context: &mut impl InitContext<Self>,
    ) -> bool {
        let status = self
            .engine
            .set_sample_rate(buffer_config.sample_rate as f64);
        if status != harmonic_core::SampleRateStatus::Ok {
            nih_plug::nih_log!(
                "harmonic_synth: sample rate {} Hz is outside the supported range (8 kHz - 768 kHz); rejecting",
                buffer_config.sample_rate
            );
            return false;
        }
        self.engine.set_hq(self.params.hq_mode.value());
        context.set_latency_samples(self.hq_latency());
        self.last_hq = self.params.hq_mode.value();
        true
    }

    fn reset(&mut self) {
        self.engine.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // Envelope times: per-block is fine, the engine ramps levels per-sample.
        self.engine.set_envelope(
            self.params.attack.value() as f64,
            self.params.release.value() as f64,
        );
        // FM ratio: stepped feel, per-block is fine.
        self.engine.set_fm(
            self.params.fm_ratio.value() as f64,
            self.params.fm_amount.value() as f64,
        );
        // Voice mode, unison, LFO — per-block.
        self.engine
            .set_free_running(self.params.free_running.value());

        // HQ mode: when it toggles, the character stage's decimation FIR
        // latency changes — tell the host so it can re-sync.
        let hq = self.params.hq_mode.value();
        self.engine.set_hq(hq);
        if hq != self.last_hq {
            context.set_latency_samples(self.hq_latency());
            self.last_hq = hq;
        }
        self.engine.set_unison(
            self.params.unison_voices.value() as u32,
            self.params.unison_detune.value() as f64,
            self.params.unison_spread.value() as f64,
        );
        self.engine.set_lfo(
            self.params.lfo_rate.value() as f64,
            self.params.lfo_shape.value().to_core(),
            self.params.lfo_to_bright.smoothed.next() as f64 * 0.5,
            self.params.lfo_vibrato.smoothed.next() as f64,
        );
        // Filter: mode + base cutoff + resonance + envelope depth, per-block.
        // Cutoff automation lands at block rate; the filter-envelope path
        // inside the engine still modulates cutoff per-sample per-voice.
        self.engine.set_filter(
            self.params.filter_mode.value().to_core(),
            self.params.filter_cutoff.value() as f64,
            self.params.filter_res.value() as f64,
            self.params.filter_env.value() as f64,
        );
        self.engine.set_filter_envelope(
            self.params.filter_env_attack.value() as f64,
            self.params.filter_env_decay.value() as f64,
            self.params.filter_env_sustain.value() as f64,
            self.params.filter_env_release.value() as f64,
        );

        let mut next_event = context.next_event();

        for (sample_id, channel_samples) in buffer.iter_samples().enumerate() {
            // Drain all MIDI events scheduled at or before this frame.
            while let Some(event) = next_event {
                if event.timing() > sample_id as u32 {
                    break;
                }
                match event {
                    NoteEvent::NoteOn { note, velocity, .. } => {
                        self.engine.note_on(note, velocity)
                    }
                    NoteEvent::NoteOff { note, .. } => self.engine.note_off(note),
                    NoteEvent::Choke { note, .. } => self.engine.choke(note),
                    NoteEvent::MidiPitchBend { value, .. } => {
                        // value ∈ [0, 1], 0.5 = centre
                        let st = (value - 0.5) * 2.0 * self.params.bend_range.value() as f32;
                        self.engine.set_pitch_bend(st as f64);
                    }
                    NoteEvent::MidiCC { cc, value, .. }
                        if cc == 123 && value <= f32::EPSILON =>
                    {
                        self.engine.all_notes_off()
                    }
                    _ => {}
                }
                next_event = context.next_event();
            }

            // Sample-accurate parameter automation.
            self.engine
                .set_rolloff(brightness_to_r(self.params.brightness.smoothed.next()));
            self.engine
                .set_gain(self.params.gain.smoothed.next() as f64);

            let grit = self.params.grit.smoothed.next();
            self.engine.set_character(CharParams {
                drive: self.params.drive.smoothed.next(),
                bias: 0.25 * self.params.drive.value(), // asymmetry rides with drive
                fold: self.params.fold.smoothed.next(),
                crush: grit,
                downsample: grit * 0.8,
            });
            self.engine
                .set_feedback(self.params.feedback.smoothed.next() as f64);

            let [l, r] = self.engine.render_sample();
            let mut ch = channel_samples.into_iter();
            if let Some(left) = ch.next() {
                *left = l;
            }
            if let Some(right) = ch.next() {
                *right = r;
            }
            for extra in ch {
                *extra = l; // mono-sum fallback for >2 channels
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for HarmonicSynth {
    const CLAP_ID: &'static str = "com.harmonic-core.harmonic-synth";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Band-limited additive synth (Dirichlet kernel)");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Synthesizer,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for HarmonicSynth {
    const VST3_CLASS_ID: [u8; 16] = *b"HarmonicSynth\0\0\0";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

nih_export_clap!(HarmonicSynth);
nih_export_vst3!(HarmonicSynth);
