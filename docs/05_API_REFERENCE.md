# 05 — Довідник API

Дві поверхні: **Rust API** (`harmonic_core::*`) для Rust-хостів (плагін
`harmonic_synth`), та **C-ABI** (`harmonic_voice_*`) для не-Rust хостів.

Конвенція викликів:
- **setup** — виклик поза аудіо-потоком або на note-on (алокацій немає, але
  деякі роблять `Svf::recompute` / `Adsr::set`);
- **RT** — безпечно посемплово в аудіо-потоці;
- **RT-fanout** — безпечно, але лінійно по кількості голосів (`PolySynth`).

---

## 1. Rust API

### `midi_to_hz`

```rust
pub fn midi_to_hz(note: f32) -> f64
```
Рівномірна темперація, `A4 (note 69) = 440 Hz`. Дробові `note` дозволені
(pitch bend, мікротони). RT.

### `struct Voice`  — один голос, стерео-вихід

`#[repr(C)] Copy`. Створення: `Voice::new(sample_rate: f64)` — `sample_rate`
клампиться в `[8000, 768000]`, інакше `48000`.

| Метод | Кон-я | Одиниці / діапазон (клампиться) |
|---|---|---|
| `set_frequency(hz: f64)` | setup | `[1, f_s/2)` Hz |
| `set_pitch_bend(ratio: f64)` | RT | `[2⁻⁵, 2⁵]` (тобто `2^(±60 семитонів/12)`) |
| `set_start_phase(turns: f64)` | setup | будь-яке; береться `frac` |
| `set_rolloff(r: f64)` | RT | `[1e-3, 0.9995]` |
| `set_gain(g: f64)` | RT | `[0, 8]` лінійно |
| `set_pan(pan: f64)` | RT | `[−1, 1]`, equal-power, згладж. ~10 мс |
| `set_free_running(free: bool)` | setup | — |
| `set_fm(ratio: f64, index: f64)` | RT | `ratio [0,64]`, `index [0,8]` обертів |
| `set_feedback(fb: f64)` | RT | `[0, 0.9]` |
| `set_lfo(rate_hz: f64, shape: LfoShape)` | setup | `rate [0, f_s/2)` |
| `set_lfo_targets(to_rolloff: f64, to_pitch_cents: f64)` | RT | `[−0.9,0.9]`, `[−1200,1200]` центів |
| `set_lfo_phase(turns: f64)` | setup | — |
| `set_character(p: CharParams)` | RT | див. `CharParams` |
| `set_filter_mode(m: FilterMode)` | setup | — |
| `set_filter_cutoff(hz: f64)` | RT | `[20, 0.45·f_s]` Hz, згладж. ~1 мс всередині |
| `set_filter_resonance(r: f64)` | RT | `[0, 1]` → `Q [0.5, 32]` |
| `reset()` | setup (note-on) | скид фази (якщо `!free_running`) + де-клік; скид згладжувачів, фільтра, LFO |
| `max_partials() -> u32` | — | `⌊f_s/(2·freq_z)⌋`, обмежено `2048` |
| `current_frequency() -> f64` | — | `freq_z · bend_z`, Hz (для метрів) |
| `render_sample() -> [f32; 2]` | RT | `[L, R]` |
| `render_block(left: &mut [f32], right: &mut [f32])` | RT | до `min(len)` |

Константи: `Voice::ROLLOFF_MIN = 1e-3`, `Voice::ROLLOFF_MAX = 0.9995`.
`voice::MAX_PARTIALS = 2048`.

### `struct PolySynth<const VOICES: usize>`

Створення: `PolySynth::<N>::new(sample_rate)`. `set_sample_rate(sr)`
перебудовує (глушить голоси).

**Тембр / рівень** (RT-fanout):
```rust
set_rolloff(r: f64)                 // [0, 1] — «brightness» через set_rolloff у Voice
set_gain(g: f64)                    // майстер, pre soft-clip
set_character(p: CharParams)
set_fm(ratio: f64, index: f64)
set_feedback(fb: f64)
```

**Режим голосу** (RT-fanout):
```rust
set_free_running(free: bool)
set_unison(count: u32, detune_cents: f64, spread: f64)   // count clamp [1,8], spread [0,1]
set_pitch_bend(semitones: f64)                            // → ratio 2^(st/12), на всі голоси
set_lfo(rate_hz: f64, shape: LfoShape, to_rolloff: f64, to_pitch_cents: f64)
```

**Обгинаючі** (RT-fanout, оновлює живі голоси):
```rust
set_envelope(attack_s: f64, release_s: f64)               // амплітудна AR (sustain=1, decay≈0)
set_amp_adsr(a: f64, d: f64, s: f64, r: f64)              // повна амплітудна ADSR
set_filter(mode: FilterMode, cutoff_hz: f64, resonance: f64, env_octaves: f64)
set_filter_envelope(a: f64, d: f64, s: f64, r: f64)       // окрема фільтрова ADSR
```
`env_octaves` — біполярна глибина фільтрової обгинаючої → cutoff, в октавах
при піку (`~[−6, 6]`). `env_octaves == 0` → cutoff статичний, посемпловий
перерахунок пропускається.

**MIDI** (setup / подієво):
```rust
note_on(note: u8, velocity: f32)   // стекає unison_count голосів
note_off(note: u8)                  // release для всіх голосів з цим note
choke(note: u8)                     // жорсткий стоп
all_notes_off()                     // release для всіх
reset()                             // жорстка тиша (host reset)
```

**Вихід** (RT):
```rust
render_sample() -> [f32; 2]
render_block(left: &mut [f32], right: &mut [f32])
active_voice_count() -> usize
```

Константа: `poly::MAX_UNISON = 8`.

### Типи-параметри

```rust
pub struct CharParams {
    pub drive: f32,      // [0,1]  — пре-гейн у сатуратор
    pub bias: f32,       // [-1,1] — асиметрія (парні гармоніки)
    pub fold: f32,       // [0,1]  — глибина вейвфолдера
    pub crush: f32,      // [0,1]  — 0 = 12-біт, 1 ≈ 2-біт
    pub downsample: f32, // [0,1]  — 0 = вимк, 1 ≈ hold кожні 16 семплів
}
pub const CharParams::CLEAN;    // усі нулі → process() це identity

#[repr(u32)] pub enum FilterMode { Bypass=0, Low=1, Band=2, High=3, Notch=4 }
impl FilterMode { pub fn from_u32(v: u32) -> Self }   // невідоме → Bypass

#[repr(u32)] pub enum LfoShape { Sine=0, Triangle=1, Saw=2 }
impl LfoShape { pub fn from_u32(v: u32) -> Self }     // невідоме → Sine
```

### Низькорівневі (експоновані для тестів / офлайн)

```rust
// kernel:
pub fn dirichlet_blit(p: f64, n: u32) -> f64
pub fn geometric_partials(p: f64, r: f64, n: u32) -> f64
pub fn geometric_peak(r: f64, n: u32) -> f64
pub fn powi_pos(base: f64, exp: u32) -> f64
pub fn geometric_partials_x4(p0: f64, dp: f64, r: f64, n: u32) -> [f64; 4]   // батч
#[cfg(feature = "portable-simd")]
pub fn geometric_partials_x4_simd(p0: f64, dp: f64, r: f64, n: u32) -> [f64; 4]

// trig:
pub fn cos_turns(turns: f64) -> f64
pub fn sin_turns(turns: f64) -> f64
pub fn sin_cos_turns(turns: f64) -> (f64, f64)     // (sin, cos)
pub fn tan_turns(turns: f64) -> f64                // |turns| < 0.25
pub fn floor_f64(x: f64) -> f64
pub fn exp2(x: f64) -> f64
pub fn cos_turns_branchless(x: f64) -> f64
pub fn cos4_turns(x: [f64; 4]) -> [f64; 4]

// Svf, Adsr, Lfo, Character — публічні структури для прямого вжитку
// поза Voice; сигнатури див. відповідні модулі.
```

---

## 2. C-ABI

Заголовок: `harmonic_core/include/harmonic_core.h`. Усі функції
`extern "C"`, `#[no_mangle]`. `Voice*` — опаковий (`typedef struct
HarmonicVoice HarmonicVoice`).

### Життєвий цикл

```c
size_t harmonic_voice_size(void);   /* ≈ 200 */
size_t harmonic_voice_align(void);  /* 8 */
void   harmonic_voice_init(HarmonicVoice *voice, double sample_rate);
/* деструктора немає — Voice це POD; викликач звільняє свою пам'ять */
```

### Сеттери (усі клампляться всередині; NULL → no-op)

```c
void harmonic_voice_set_frequency  (HarmonicVoice*, double hz);          /* [1, fs/2) */
void harmonic_voice_set_rolloff    (HarmonicVoice*, double r);           /* [1e-3, 0.9995] */
void harmonic_voice_set_gain       (HarmonicVoice*, double g);           /* [0, 8] */
void harmonic_voice_set_pan        (HarmonicVoice*, double pan);         /* [-1, 1] equal-power */
void harmonic_voice_set_pitch_bend (HarmonicVoice*, double semitones);   /* → 2^(st/12) */
void harmonic_voice_set_free_running(HarmonicVoice*, unsigned int on);   /* 0 = reset+declick */
void harmonic_voice_set_filter(HarmonicVoice*, unsigned int mode,        /* 0..4 */
                               double cutoff_hz, double resonance);      /* [20,0.45fs] [0,1] */
void harmonic_voice_set_lfo(HarmonicVoice*, double rate_hz, unsigned int shape, /* 0..2 */
                            double to_rolloff, double to_pitch_cents);
```

### Робота

```c
void   harmonic_voice_reset(HarmonicVoice *voice);   /* note-on; поважає free_running */

/* num_frames INTERLEAVED-стерео семплів: out повинен вмістити 2*num_frames
   float, розкладку L R L R ...  RT-safe. */
void   harmonic_voice_process(HarmonicVoice *voice, float *out, size_t num_frames);

double harmonic_voice_current_frequency(const HarmonicVoice *voice);  /* freq*bend, Hz; NULL→0 */
```

### Приклад (аудіо-callback)

```c
static HarmonicVoice *v;               /* ініціалізовано один раз */

void audio_callback(float *stereo_out, int frames) {
    harmonic_voice_process(v, stereo_out, frames);   /* stereo_out: 2*frames float */
}

void on_note(int midi_note) {
    harmonic_voice_set_frequency(v, 440.0 * pow(2.0, (midi_note - 69) / 12.0));
    harmonic_voice_reset(v);
}
```

### Потокобезпека

C-ABI **не** потокобезпечний. Не викликайте сеттери та
`harmonic_voice_process` одночасно з різних потоків без зовнішньої
синхронізації. (Плагін `harmonic_synth` не використовує C-ABI — він
викликає `PolySynth` напряму, а потокобезпеку GUI→аудіо забезпечує
`nih-plug`.)

---

## 3. Параметри плагіна `harmonic_synth`

27 параметрів (host-generic UI, без власного GUI). Групи:

| Група | Параметри |
|---|---|
| Тон | Brightness, Gain |
| Амплітудна обгинаюча | Attack, Release |
| Character | Drive, Fold, Grit |
| FM | FM Amount, FM Ratio, Feedback |
| Фільтр | Filter (enum Off/LP/BP/HP/Notch), Cutoff, Resonance |
| Фільтрова обгинаюча | Filter Env (± окт), F.Env Attack/Decay/Sustain/Release |
| Режим голосу | Free-Run Phase |
| Унісон | Unison (1–8), Uni Detune (ct), Uni Spread (%) |
| Модуляція | Bend Range (st), LFO Rate, LFO Shape, LFO → Bright, LFO Vibrato (ct) |

`Grit` мапиться на `crush` + `downsample·0.8` разом. `Bend Range` мапить
`MidiPitchBend` value `[0,1]` → `(value−0.5)·2·range` семитонів.
