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
| `set_lfo_mode(m: LfoMode)` | setup | `Retrigger` (фаза → 0 на note-on) / `FreeRun` |
| `set_lfo_targets(to_rolloff, to_pitch_cents, to_cutoff_oct, to_fm)` | RT | `[−0.9,0.9]` · `[−1200,1200]` центів · `[−8,8]` окт · `[−8,8]`; кожна `0` = не застосовується |
| `set_lfo_phase(turns: f64)` | setup | — |
| `set_unison_drift(rate_hz: f64, depth_turns: f64)` | setup | `rate [0,5]` Гц · `depth [0,0.25]` обертів; обидва `0` = вимк |
| `set_unison_drift_phase(turns: f64)` | setup | стартова фаза дрейфу (декореляція голосів) |
| `set_character(p: CharParams)` | RT | див. `CharParams` |
| `set_hq(hq: bool)` | setup | 2× оверсемплінг осц.+character; `true` додає `Voice::HQ_LATENCY` (=3) семпли; лише для `Waveform::Geometric` |
| `set_waveform(w: Waveform)` | setup | `Geometric` / `Saw` / `Triangle`; Saw+Triangle — PolyBLEP/PolyBLAMP, ігнорують `rolloff` та HQ |
| `set_partial_limit(limit: f32)` | setup | стеля на гармоніки, `[1.0, 2048.0]`, фракційна (гладкий свіп), деф. 2048 = без ефекту; після Найквіст-клампу (не аліасить), плоска вартість; лише `Waveform::Geometric` (`04 §0`) |
| `set_expr_brightness(r_offset: f64)` | RT | понотний зсув `rolloff` (MPE / афтертач), `[−0.9, 0.9]`, згладж. ~5 мс, поверх LFO→brightness; `0.0` = побайтова тотожність; лише `Geometric` (`04 §0.1`) |
| `set_formant(f: f64)` | RT | резонансний горб у середніх партіалах, `[0, 1]`, згладж. ~5 мс; `0.0` = побайтова тотожність; лише `Geometric` (`04 §0.2`) |
| `set_filter_mode(m: FilterMode)` | setup | — |
| `set_filter_cutoff(hz: f64)` | RT | `[20, 0.45·f_s]` Hz, згладж. ~1 мс всередині |
| `set_filter_resonance(r: f64)` | RT | `[0, 1]` → `Q [0.5, 32]` |
| `reset()` | setup (note-on) | скид фази (якщо `!free_running`) + де-клік; скид згладжувачів, фільтра; LFO ретригериться лише в режимі `Retrigger` |
| `render_sample() -> [f32; 2]` | RT | `[L, R]` |
| `render_block(left: &mut [f32], right: &mut [f32])` | RT | до `min(len)` |
| `max_partials() -> u32` | — | ефективна кількість гармонік: `min(⌊f_s/(2·freq_z)⌋, 2048, ⌊partial_limit⌋)` |
| `current_frequency() -> f64` | — | `freq_z · bend_z`, Hz (для метрів) |
| `current_cutoff() -> f64` | — | живий згладжений зріз фільтра (env + LFO згорнуті), Hz — для дисплея |
| `sample_rate() -> f64` | — | валідована частота дискретизації голосу |

Конструктори: `Voice::new(sr)` (клампить тихо) або
`Voice::new_checked(sr) -> (Voice, SampleRateStatus)` — повертає статус
(`Ok`/`ClampedLow`/`ClampedHigh`/`Defaulted`).

Константи: `Voice::HQ_LATENCY = 3`, `Voice::ROLLOFF_MIN = 1e-3`,
`Voice::ROLLOFF_MAX = 0.9995`, `voice::MAX_PARTIALS = 2048`.

### `struct PolySynth<const VOICES: usize>`

Створення: `PolySynth::<N>::new(sample_rate)` (клампить тихо) або
`new_checked(sr) -> (Self, SampleRateStatus)`. `set_sample_rate(sr) ->
SampleRateStatus` перебудовує (глушить голоси) і повертає статус — плагін
**не** відхиляє ініціалізацію на не-`Ok`: клампить, пише в лог і завжди
повертає `true` з `initialize` (`harmonic_synth/src/lib.rs`; `07 §11`
має причину). `sample_rate() -> f64` — поточна валідована.
Константа: `PolySynth::<N>::HQ_LATENCY = 16` — латентність Unified HQ Bus
(`04 §1.7`), незалежна від `Voice::HQ_LATENCY` (=3, лише для прямого
`Voice`/C-ABI шляху нижче).

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
set_hq(hq: bool)                                          // Unified HQ Bus; +PolySynth::HQ_LATENCY (=16) семплів
set_waveform(w: Waveform)                                 // Geometric / Saw / Triangle
set_partial_limit(limit: f32)                            // стеля на гармоніки [1.0,2048.0], фракційна, деф. 2048; після Найквіста; плоска вартість
set_brightness_depth(depth: f64)                         // глибина понотної яскравості, [−0.9,0.9] r-одиниць; 0.0 вимикає (побайт. тотожн.)
set_formant(f: f64)                                      // резонансний горб, [0,1]; 0.0 вимикає (побайт. тотожн.)
set_note_brightness(note: u8, raw: f32)                  // MPE-тембр+поліафтертач для клавіші note; wildcard 255 ігнор.
set_channel_brightness(raw: f32)                         // тиск каналу — спільно на всі звучні ноти (04 §0.1)
set_unison(count: u32, detune_cents: f64, spread: f64, drift: f64)  // clamp [1,8] · [0,1] · [0,1]
set_pitch_bend(semitones: f64)                            // → ratio 2^(st/12), на всі голоси
set_lfo(rate_hz, shape: LfoShape, mode: LfoMode,
        to_rolloff, to_pitch_cents, to_cutoff_oct, to_fm)   // 0 = target off
set_tuning(t: Tuning)                                     // мікротюнінг; діє з наступного note-on
set_tuning_equal()                                       // → 12-TET / A4=440, побайтовий дефолтний шлях
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
lowest_sounding_hz() -> f64        // фундаментал найнижчої звучної ноти під тюнінгом; 0 = тихо. Для дисплея, не на рендер-шляху
representative_cutoff() -> f64      // живий зріз фільтра тієї ж (найнижчої) ноти, env+LFO згорнуті; 0 = тихо. Для живої кривої фільтра
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

#[repr(u32)] pub enum LfoMode { Retrigger=0, FreeRun=1 }
impl LfoMode { pub fn from_u32(v: u32) -> Self }      // невідоме → Retrigger

#[repr(u32)] pub enum Waveform { Geometric=0, Saw=1, Triangle=2 }
impl Waveform { pub fn from_u32(v: u32) -> Self }     // невідоме → Geometric

pub struct Tuning { /* Copy; period + degree cents + (ref_note, ref_hz) anchor */ }
impl Tuning {
    pub const MAX_DEGREES: usize = 64;   // 53-EDO / Turkish-53 fit; більше — обрізає викликач
    pub const EQUAL_440: Tuning;                              // 12-TET, A4=440 (дефолт)
    pub fn equal(edo: u8, ref_hz: f64, ref_note: u8) -> Tuning;      // n рівних поділів октави
    pub fn from_cents(cents: &[f64], period: f64, ref_hz: f64, ref_note: u8) -> Tuning; // довільна Scala-шкала
    pub fn is_equal_440(&self) -> bool;                       // → true вмикає дефолтний fast path у PolySynth
    pub fn hz(&self, note: u8) -> f64;                        // завжди скінченна > 0
}
```

Усі конструктори `Tuning` санітизують вхід (NaN-центи → 0, період → `[1, 4800]`,
`ref_hz` → `[8, 20000]`, `edo`/довжина → `[1, MAX_DEGREES]`) — значення завжди
придатне. Мапінг клавіатури лінійний: MIDI-нота `ref_note` = ступінь 0, кожна
вища клавіша — наступний ступінь, із переходом у наступний період.

`Saw` / `Triangle` — **PolyBLEP / PolyBLAMP** (Välimäki & Huovilainen 2007),
**без стану**. Фіксовані спектри `1/k` / `1/k²` (`rolloff` і HQ ігноруються),
амплітуда `±1`, відгук **плаский до DC**. Деталі — `01_MATHEMATICS.md` §7,
`04_DSP_COMPONENTS.md` §11.

### Низькорівневі (експоновані для тестів / офлайн)

```rust
// kernel:
pub fn dirichlet_blit(p: f64, n: u32) -> f64
pub fn geometric_partials(p: f64, r: f64, n: u32) -> f64
pub fn geometric_partials_pre(p: f64, r: f64, n: u32, rn1: f64) -> f64   // rn1 = powi_pos(r, n+1), кешується
pub fn geometric_peak(r: f64, n: u32) -> f64
pub fn geometric_peak_pre(r: f64, n: u32, rn: f64) -> f64               // rn = powi_pos(r, n), кешується
pub fn powi_pos(base: f64, exp: u32) -> f64
pub fn geometric_partials_x4(p0: f64, dp: f64, r: f64, n: u32) -> [f64; 4]   // батч
#[cfg(feature = "portable-simd")]
pub fn geometric_partials_x4_simd(p0: f64, dp: f64, r: f64, n: u32) -> [f64; 4]

// trig:
pub fn cos_turns(turns: f64) -> f64
pub fn sin_turns(turns: f64) -> f64
pub fn sin_cos_turns(turns: f64) -> (f64, f64)     // (sin, cos)
pub fn tan_turns(turns: f64) -> f64                // |turns| < 0.25
pub fn tan_turns_fast(turns: f64) -> f64           // turns ∈ [0, 0.23] — [3/2] rational, ~4× cheaper
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
size_t harmonic_voice_size(void);   /* 624 — не хардкодити, зростає з версіями */
size_t harmonic_voice_align(void);  /* 8 */
int    harmonic_voice_init(HarmonicVoice *voice, double sample_rate);
       /*  0 ok · 1 clamped-low · 2 clamped-high · 3 defaulted (NaN/inf) · -1 null */
double harmonic_voice_sample_rate(const HarmonicVoice *voice);  /* фактична (після клампу) */
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
void harmonic_voice_set_lfo(HarmonicVoice*, double rate_hz,
                            unsigned int shape,   /* 0 sine / 1 tri / 2 saw */
                            unsigned int mode,    /* 0 retrigger / 1 free-run */
                            double to_rolloff, double to_pitch_cents,
                            double to_cutoff_oct, double to_fm);  /* 0 = target off */
void harmonic_voice_set_hq(HarmonicVoice*, unsigned int hq);  /* !=0 → 2× OS, +3 семпли латентності */
void harmonic_voice_set_waveform(HarmonicVoice*, unsigned int waveform); /* 0 geom / 1 saw / 2 tri */
void harmonic_voice_set_partial_limit(HarmonicVoice*, float limit); /* [1.0,2048.0] fractional, 2048=none; after Nyquist, no alias, flat cost */
void harmonic_voice_set_expr_brightness(HarmonicVoice*, double r_offset); /* понотний зсув rolloff (MPE/афтертач), [-0.9,0.9], 0.0=тотожність */
void harmonic_voice_set_formant(HarmonicVoice*, double f);              /* резонансний горб у середніх партіалах, [0,1], 0.0=тотожність */
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

36 параметрів. Редактор — `nih_plug_vizia` (`src/editor.rs`): заголовок +
рядок пресетів (`◀ ім'я ▶`) + живий спектр-дисплей (банк band-pass `Svf`,
не FFT; поверх — гребінка партіалів закритої форми, крива відгуку фільтра,
hover-«розбери пресет») + чесний метр аліасингу (вузький BP на `0.44·f_s`, кольорова смуга
праворуч — `04 §1.8`) + рядок A/B-морфу + рядок seed-рандомайзера + згруповані
секції параметрів (8: TONE / AMP ENVELOPE / CHARACTER / FM / FILTER / VOICE /
TUNING / MODULATION; `ParamSlider` + `ParamButton`) у `ScrollView` — замість плоского
`GenericUi`.

**Стартовий банк пресетів** (`src/presets.rs`, ~22) — кожен пресет це набір
оверрайдів у **plain-одиницях** (с, Гц, дБ, відношення, `0..1`) поверх
дефолтів. Лоадер (`editor`) для кожного параметра скидає в дефолт або бере
оверрайд, конвертує plain → нормалізовано (`ParamPtr::preview_normalized`) і
шле як звичайну автоматизацію (той самий шлях, що морф / рандом). Індекс
персиститься (`#[persist] preset_idx: i32`, `-1` = змінено). Тести
`presets::tests` перевіряють, що кожен пресет рендерить обмежене, не-тихе
аудіо з правильним широким нахилом.

**Seed-рандомайзер** (`src/rando.rs`) — `RANDOM` котить 30-бітний seed і пише
патч у справжні параметри (той самий `RawParamEvent`-шлях, що й морф); seed
показується як **6-символьний код** (Crockford base-32), ввід коду відтворює
патч **побайтово на будь-якій машині** (мапінг — чистий SplitMix64,
цілочисельний). Вікна рандомізації на параметр — `rando::SPEC` (навмисно
вужчі за повний діапазон, щоб частіше траплявся придатний звук); `HQ Mode`,
`Bend Range`, `Free-Run` не чіпаються. Seed персиститься (`#[persist] seed:
u32`, `0` = немає); «пливе» від звуку, щойно крутнеш ручку. Межі — `07 §19`.

**A/B морф** — не параметр, а редакторний інструмент. `SET A` / `SET B`
знімають нормалізовані значення всіх 39 параметрів у персистовані слоти
(`#[persist] morph_a` / `morph_b` — `Vec<(id, f32)>`); слайдер пише
`lerp(A, B, pos)` у справжні параметри через `RawParamEvent`
(`Begin/SetNormalized/End` на кожен). Позиція теж персиститься
(`morph_pos: f32`). `pos = 0` дає рівно A, `pos = 1` рівно B (побітово).
Кожен параметр інтерполюється; дискретні (Oscillator / Filter / LFO Shape /
HQ / Free-Run) перемикаються на `pos = 0.5`. Межі — `07 §18`.

Розмір вікна персиститься (`#[persist] editor_state: Arc<ViziaState>`). Групи
параметрів:

| Група | Параметри |
|---|---|
| Тон | **Oscillator** (enum Geometric/Saw/Triangle), Brightness, **Partials** (фракційна стеля на гармоніки, `04 §0`), **Expr → Bright** (понотна яскравість MPE / афтертач → `rolloff`, біполярна, `04 §0.1`), **Formant** (резонансний горб, `04 §0.2`), Gain |
| Амплітудна обгинаюча | Attack, Release |
| Character | Drive, Fold, Grit (`bias` — не окремий слайдер; `CharParams::bias = 0.25·drive`, свідомо прив'язаний до Drive, щоб не роздувати список параметрів) |
| FM | FM Amount, FM Ratio, Feedback |
| Фільтр | Filter (enum Off/LP/BP/HP/Notch), Cutoff, Resonance |
| Фільтрова обгинаюча | Filter Env (± окт), F.Env Attack/Decay/Sustain/Release |
| Режим голосу | Free-Run Phase, **HQ Mode** (Unified HQ Bus — **+16 семплів** латентності, `PolySynth::HQ_LATENCY`, PDC повідомляється константно; НЕ плутати з `Voice::HQ_LATENCY = 3`, яка стосується лише прямого C-ABI, не плагіна) |
| Унісон | Unison (1–8), Uni Detune (ct), Uni Spread (%), **Uni Drift** (%) |
| **Мікротюнінг** | **Tuning** (enum: Equal / Just Intonation / Pythagorean / 1/4-comma Meantone / 19-EDO / 24-EDO / 31-EDO / Bohlen-Pierce), **Tune Root** (0–11, тоніка 12-нотних історичних шкал; ігнор. для Equal / EDO), **Tune Ref** (415–467 Гц, A4; `440` = стандарт), **Scala** (поле в редакторі: шлях до `.scl` або вставлений вміст → перекриває Tuning enum) |
| Модуляція | Bend Range (st), LFO Rate, LFO Shape, **LFO Sync** (Retrigger/Free-Run), LFO → Bright, LFO Vibrato (ct), **LFO → Cutoff** (±4 окт), **LFO → FM** (±4) |

`Grit` мапиться на `crush` + `downsample·0.8` разом. `Bend Range` мапить
`MidiPitchBend` value `[0,1]` → `(value−0.5)·2·range` семитонів.

**Мікротюнінг** (`harmonic_synth/src/tuning.rs`): `Equal` при `Tune Ref = 440`
дає `PolySynth::set_tuning_equal()` → побайтовий дефолтний шлях (нічого не
змінюється). Решта збирає `Tuning` і викликає `set_tuning` **лише коли** щось
у сигнатурі `(enum, root, ref, FNV-хеш Scala-рядка)` зрушилось (`process`
тримає `tuning_sig`). Ретюнить **фундаментал** ноти; обертони лишаються на
`k·f0` (закрита форма — `07 §20`). Не входить у seed-рандом. Шкала діє з
наступного note-on.

**Scala-імпорт.** `#[persist] scala: Arc<Mutex<String>>` тримає компактну
форму `"<period>;<c0>,<c1>,…"` (порожня = enum). Редактор (`ScalaEvent::Load`):
`tuning::load` читає `.scl`-файл за шляхом **або** парсить вставлений вміст
(відношення `3/2` / центи `701.955`, `!`-коментарі), `to_compact` → у
`params.scala`; `✕` очищає. `process` бере мьютекс через **`try_lock`** (не
блокує аудіо-потік), парсить компактну форму **без алокацій** (`expand_compact`
→ `[f64; 64]` на стеку) → `build_scala` → `set_tuning`. `.kbm` (мапінг
клавіатури) та MTS-ESP — ще ні (`09`).

`MIDI_INPUT = MidiConfig::MidiCCs` (не `Basic`): обгортки nih-plug (VST3 і
CLAP) віддають події MIDI CC, pitch-bend і channel-pressure **лише** з цього
рівня. Це живить колесо висоти, педаль CC#64, CC#123 all-notes-off **і**
понотну експресію (MPE-тембр / поліафтертач / тиск каналу → «Expr → Bright»).
Для VST3 реєструються 130×16 хостових CC-параметрів (штатний механізм
nih-plug); у збережений стан плагіна вони не входять.
