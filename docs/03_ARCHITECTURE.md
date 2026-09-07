# 03 — Архітектура

Два крейти:

- **`harmonic_core`** — `#![no_std]`-сумісний DSP-рушій, нуль залежностей,
  C-ABI. Тут уся математика й синтез.
- **`harmonic_synth`** — плагін VST3 + CLAP над `harmonic_core`, через
  `nih-plug` + `nih_plug_vizia`. Хостовий клей: параметри, маршрутизація
  MIDI, семпловий цикл; редактор (`src/editor.rs`) та дешевий спектр-дисплей
  на банку band-pass фільтрів (`src/analyzer.rs`).

---

## 1. Граф модулів `harmonic_core`

```
trig ──────────────┬──────────────┬───────────┬──────────┐
 (sin/cos/exp2/     │              │           │          │
  tan/floor +       ▼              ▼           ▼          ▼
  cos4 branchless) kernel        filter       env        lfo
                  (D_n, S_n,    (ZDF SVF)   (ADSR)    (sine/tri/saw)
                   x4 batch)      │           │          │
                     │           └─────┬──────┴────┬─────┘
                     │                 ▼           │
                     └──────────────▶ voice ◀──────┘
                                   (осцилятор + FM + LFO +
                                    character + SVF + pan +
                                    pitch bend + de-click)
                                        │
                        ┌───────────────┼───────────────┐
                        ▼               ▼               ▼
                      poly            ffi          (rustdoc)
                 (PolySynth<N>:    (C-ABI, володіє
                  унісон, стіл,     викликач)
                  bend, LFO,
                  2× ADSR)
                        │
                        ▼
                 harmonic_synth (nih-plug)
```

Правило залежностей: модуль залежить лише від того, що вище за ним. `voice`
— точка збірки; `poly` — поліфонічна обгортка; `ffi` — паралельна поверхня
для не-Rust хостів.

| Модуль | Рядків* | Роль |
|---|---|---|
| `trig` | ~230 | Тригонометрія `no_std`, `exp2`, branchless-батч |
| `kernel` | ~230 | `dirichlet_blit`, `geometric_partials`, `geometric_peak`, `geometric_partials_x4` |
| `character` | ~230 | drive / bias / fold / crush / downsample |
| `filter` | ~250 | `Svf` — ZDF SVF з посемпловим згладжуванням параметрів |
| `env` | ~160 | `Adsr` |
| `lfo` | ~180 | `Lfo` (+ `LfoMode`) |
| `voice` | ~370 | `Voice` — повний тракт одного голосу, стерео-вихід |
| `poly` | ~460 | `PolySynth<VOICES>` |
| `tuning` | ~300 | `Tuning` — нота→частота: 12-TET (дефолт, побайтово = `midi_to_hz`), n-EDO, довільна Scala-шкала, Scala `.kbm` клавіатурна мапа (мертві клавіші) |
| `ffi` | ~230 | C-ABI (+ `wasm32`-only статичне сховище `harmonic_wasm_*`) |
| `verify` | ~150 | канонічний крос-платформний рендер + хеш (тест + ARM-звірка + wasm-звірка); `wasm32` експорти `hc_verify_*` |

\* приблизно, включно з докстрінгами й тестами.

---

## 2. `Voice` — стан одного голосу

`#[repr(C)] #[derive(Clone, Copy)]`, **624 байт** (x86-64; `align = 8`), без
`Drop`, без вказівників. Розмір може змінюватись між версіями — хост
**обов'язково** викликає `harmonic_voice_size()` у рантаймі, не хардкодить
число.

```rust
pub struct Voice {
    sample_rate: f64,
    // --- висота ---
    phase: f64,          // фаза несучої, оберти [0,1)
    freq: f64, freq_z: f64,          // ціль / згладжено (Hz)
    bend: f64, bend_z: f64,          // pitch-bend ratio, ціль / згладжено
    // --- тембр ---
    rolloff: f64, rolloff_z: f64,    // r, ціль / згладжено
    gain: f64,
    smooth_coeff: f64,               // одно-полюс ~5 мс для freq/rolloff/bend
    // --- панорама ---
    pan: f64, pan_z: f64,            // −1..1, ціль / згладжено
    pan_smooth: f64,                 // одно-полюс ~10 мс
    pan_cache_z, pan_sin, pan_cos: f64,   // кеш equal-power гейнів для сталого pan_z
    // --- старт ноти ---
    free_running: bool,              // true = фаза переживає note-on
    declick: u16,                    // семплів у fade-in, що лишились
    // --- FM ---
    fm_phase: f64, fm_ratio: f64, fm_index: f64,
    feedback: f64, last_osc: f64,
    // --- LFO ---
    lfo: Lfo,                        // + LfoMode (Retrigger / FreeRun)
    lfo_to_rolloff, lfo_to_pitch: f64,   // ± до r · центи вібрато
    lfo_to_cutoff, lfo_to_fm: f64,       // ± октави cutoff · ± FM index
    filter_cutoff: f64,             // остання база cutoff (для lfo_to_cutoff)
    drift_phase, drift_inc, drift_depth: f64,   // повільний дрейф фази (дихання унісону)
    hq: bool,                        // 2× оверсемплінг осц.+character
    // --- кеш нормалізації geometric-осцилятора (fast path) ---
    geom_r: f64, geom_n: u32,        // ключ кешу (r, n)
    geom_rn1: f64, geom_peak: f64,   // r^{n+1} та пік — обидва powi_pos пропускаються на сталій ноті
    waveform: Waveform,              // Geometric (дефолт) / Saw / Triangle
                                     // Saw/Triangle — PolyBLEP/PolyBLAMP, без стану
    partial_limit: u32, partial_frac: f32,   // фракційна стеля на гармоніки (деф. 2048.0 = без ефекту)
    expr_bright: f64, expr_bright_z: f64,     // понотний зсув rolloff (MPE/афтертач); 0.0 = тотожність
    formant: f64, formant_z: f64,             // глибина резонансного горба (04 §0.2); 0.0 = тотожність
    hump_f: f64, hump_n: u32, hump: Hump,     // кеш (a, b, h, aⁿ⁺¹, bⁿ⁺¹, peak) для (formant_z, n)
    // --- нелінійні стадії ---
    character: Character,            // включно з DC-blocker + S&H стан
    filter: Svf,                     // коеф. a1/a2/a3/k + інтегратори ic1/ic2
}
```

Атоміків **немає** — `Voice` це plain data. Потокобезпека — контракт
викликача (`ffi`) або фреймворку (`poly` → `nih-plug`).

### Сигнальний тракт `Voice::render_sample() -> [f32; 2]`

```
згладити freq_z, rolloff_z, bend_z, pan_z (одно-полюс)
  │
lfo_routed = (lfo_to_rolloff|pitch|cutoff|fm) != 0
m = if lfo_routed { LFO.tick() ∈ [−1,1] } else { 0 }   [не тикається якщо нероутований]
  ├─ f_eff    = freq_z · bend_z · [2^(lfo_to_pitch·m/1200) якщо ≠ 0]
  ├─ roll_eff = [clamp(rolloff_z + lfo_to_rolloff·m, …) якщо ≠ 0, інакше rolloff_z]
  ├─ fm_index_eff = [max(fm_index + lfo_to_fm·m, 0) якщо ≠ 0]
  └─ якщо lfo_to_cutoff ≠ 0: filter.set_cutoff(filter_cutoff · 2^(lfo_to_cutoff·m))
  │
n = min(⌊f_s / (2·f_eff)⌋, 2048, ⌊partial_limit⌋)      [Найквіст-кламп, тоді користувацька стеля]
frac = if ⌊partial_limit⌋ < ⌊f_s/(2·f_eff)⌋ { partial_frac } else { 0 }  [дробова стеля лише коли не зв'язує Найквіст]
(geom-кеш: r^{n+1} та пік перераховуються лише коли (roll_eff, n) змінились)
  │
drift = drift_depth·sin_turns_fast(drift_phase++)     [повільний дрейф фази, якщо depth≠0]
pm = fm_index_eff·sin_turns(fm_phase) + feedback·last_osc + drift  [фазова модуляція]
osc = match waveform {                                 [Geometric — дефолт]
        Geometric => [S_n(phase+pm) + frac·rⁿ⁺¹·cos(2π(n+1)(phase+pm))] / peak
                     │  (frac=0 → тотожно geometric_partials_pre; rn1, peak із
                     │   geom-кешу; hq → 2× оверсемпл + децимація)
        Saw       => polyblep_saw(phase+pm, step)       [наївний ramp + BLEP]
        Triangle  => polyblamp_triangle(phase+pm, step) [наївний tri + BLAMP кутів]
      }
last_osc ← osc
  │
shaped   = character.process(osc)                      [identity якщо clean; hq → process_hq]
filtered = filter.process(shaped)                      [identity якщо Bypass; cutoff/res згладжено ВСЕРЕДИНІ]
  │
dg   = declick-рампа (16 семплів, 1/16 → 1)
mono = filtered · gain · dg
  │
advance fm_phase (+= f_eff·fm_ratio/f_s, wrap)
advance phase    (+= f_eff/f_s, wrap)
  │
(sin_p, cos_p) = sin_cos_turns_fast(...)  [equal-power; кеш → лише коли pan_z рухається]
return [mono·cos_p, mono·sin_p]                         [L, R]
```

---

## 3. `PolySynth<const VOICES: usize>`

```rust
pub struct PolySynth<const VOICES: usize> {
    voices: [PolyVoice; VOICES],   // core::array::from_fn — без алокації
    // глобальні цілі, що фанаутяться в голоси при note_on та live-сеттерах:
    rolloff, gain,
    amp_a/d/s/r,                   // амплітудна ADSR
    character, fm_ratio, fm_index, feedback, free_running,
    waveform, partial_limit,       // осцилятор: форма + фракційна стеля на гармоніки (f32)
    filter_mode, filter_cutoff, filter_res, filter_env,   // env_octaves
    fenv_a/d/s/r,                  // фільтрова ADSR
    unison_count, unison_detune, unison_spread, unison_drift,
    bend_ratio, lfo_rate, lfo_shape, lfo_mode,
    lfo_to_rolloff, lfo_to_pitch, lfo_to_cutoff, lfo_to_fm,
    tuning: Tuning, tuning_default: bool,   // нота→частота; default=true → note_hz обходить на midi_to_hz
    counter: u64,                  // вік голосу для стилінгу
}

struct PolyVoice { core: Voice, amp: Adsr, filt_env: Adsr, note: u8, velocity: f32, age: u64 }
```

**Розподіл голосів** (`pick_voice`, у порядку пріоритету):
1. будь-який вільний (`!amp.is_active()`);
2. найстаріший у стадії release;
3. вкрасти глобально найстаріший.

**Мікротюнінг** (`set_tuning` / `set_tuning_equal`): `note_hz(note)` дає
частоту тоніки/ноти. Поки `tuning_default` (12-TET, A4=440) — це **побайтово**
`midi_to_hz(note)` (тест `tuning::equal_440_is_bit_identical_to_midi_to_hz`).
`Tuning` = період у центах + центи кожного ступеня + якір (ref-нота, ref-Hz) +
**клавіатурна мапа** (`keymap[k]` = ступінь для `k`-ї клавіші патерну, `-1` =
мертва клавіша; дефолт — тотожня мапа `k→k`, і тоді `hz` **побайтово** = старий
лінійний шлях). `hz(n)` = `ref_hz · 2^((formal·⌊rel/M⌋ + degrees[keymap[rel mod
M]] − c_ref) / 1200)`, де `c_ref` — центи якірної ноти (на дефолтному шляху
рівно `0.0`, тож формула згортається до `ref_hz · 2^(c/1200)`). `Tuning::from_kbm`
приймає Scala `.kbm`; `is_mapped(n)` / `PolySynth::note_is_mapped(n)` кажуть, чи
клавіша жива — `note_on` мовчки ігнорує мертві (гейт за `tuning_default`, тож
12-TET шлях недоторканий). Шкала діє з **наступного** note-on (звучні ноти не
ретюняться). `06 §2`, `07 §20`.

**Унісон** (`note_on`): `n = clamp(unison_count, 1, 8)` голосів на одну
ноту, кожен `i` отримує:
- детюн `unison_detune · (2i/(n−1) − 1)` центів → `freq · 2^(cents/1200)`;
- панораму `unison_spread · (2i/(n−1) − 1)`;
- стартову фазу та фазу LFO `i/n` (декореляція);
- швидкість `velocity · 1/√n` (make-up).

**Модуляція на всі звучні голоси:** `set_pitch_bend(semitones)`,
`set_lfo(rate, shape, →rolloff, →pitch)`, `set_character`, `set_fm`,
`set_feedback`, `set_free_running`.

**Фільтрова обгинаюча:** якщо `filter_env ≠ 0`, `render_sample` посемплово
на кожен активний голос: `v.core.set_filter_cutoff(base · 2^(filter_env · fe))`,
де `fe` — рівень фільтрової ADSR.

**Вихід:** `render_sample() -> [f32; 2]` — сума голосів (L/R окремо) ×
`gain`, потім `soft_clip` покомпонентно.

---

## 4. Типи даних

| Межа | Тип | Причина |
|---|---|---|
| Уся фазова / спектральна математика | `f64` | Точність фази при `k·p` до ~2048 обертів |
| Вхід/вихід character, SVF, обгинаючих | `f32` | Достатньо (24-бітне аудіо ≈ `6·10⁻⁸`), швидше |
| Аудіо-буфер | `f32` | Стандарт хостів |
| Лічильники гармонік | `u32` | `n ≤ 2048` |
| Вік голосу | `u64` | Практично не переповнюється |

---

## 5. Контракт RT-safety

Усередині `render_sample` / `render_block` / `harmonic_voice_process`
**суворо немає**:

- алокацій купи (`Box`, `Vec`, `String`, `format!`, ...);
- блокувань (`Mutex`, `RwLock`, атомарних spin-loop);
- I/O, файлової системи, мережі, системного часу;
- шляхів до паніки — `grep` не знаходить `unwrap`/`expect`/`panic!` у
  не-тестовому коді; усі ділення захищені за побудовою
  (`04_DSP_COMPONENTS.md`, `07_LIMITATIONS.md`);
- `panic = "abort"`.

У плагіні ввімкнена дефолтна фіча `nih-plug` **`assert_process_allocs`** —
будь-яка алокація в `process()` панікує в хості (рантайм-контроль).

**`panic = "abort"` заданий у `[profile.release]` ОБОХ `Cargo.toml`** —
`harmonic_core/` і `harmonic_synth/` окремо. Cargo застосовує `[profile.*]`
лише з кореня активного workspace, а це два різні корені: без окремого рядка
в `harmonic_synth/Cargo.toml` зібраний плагін успадковував би `panic=unwind`.
Це важливо, бо `extern "C"` точки входу nih-plug (`process`/`activate` у
`vendor/nih-plug/src/wrapper/`) **не мають** `catch_unwind` — паніка на
аудіо-шляху інакше розкручувалася б крізь C-межу, що є UB. Той самий
принцип, що й у `no_std`-панік-хендлері `harmonic_core` («зависання
безпечніше за розкрутку крізь C ABI»).

---

## 6. Матриця збірки

| Ціль | Команда | Що виходить |
|---|---|---|
| Розробка / тести | `cargo test` | `std` (дефолт), 108 тестів (90 юніт + 18 інтеграційних) |
| Bit-exact на ARM | `harmonic_core/scripts/cross-verify.sh` | Docker + QEMU: `aarch64` + `armv7-hf`, 108/108, хеш = x86-64 |
| Приклади (WAV) | `cargo run --example <name> --release` | `*.wav` у теці крейта |
| **Справжній `no_std`** | `cargo build --no-default-features --release` | `cdylib` + `staticlib`, нуль `libc`-math, `panic=abort` |
| Явний SIMD | `cargo +nightly build --features portable-simd` | `#![feature(portable_simd)]` |
| Плагін | `cd harmonic_synth && cargo xtask bundle harmonic_synth --release` | `target/bundled/harmonic_synth.{vst3,clap}`, `panic=abort` (окремо заданий у `harmonic_synth/Cargo.toml`, §5) |

`no_std`-збірка **тільки в `--release`** — dev-профіль потребує
`eh_personality` (unwind), а `panic=abort` заданий лише для релізу. Це
стандартна практика для embedded-крейтів.

---

## 7. Модель володіння FFI

`Voice` — POD, тому C-ABI не має `create`/`destroy`:

```c
size_t sz  = harmonic_voice_size();     // 624 сьогодні — НЕ хардкодити
size_t al  = harmonic_voice_align();    // 8
void  *mem = aligned_alloc(al, sz);     // викликач розміщує (стек / арена / купа)
harmonic_voice_init(mem, 48000.0);      // ptr.write(Voice::new(sr)) на місці
// ... set_* ...
harmonic_voice_process(mem, buf, 128);  // buf: 256 float, interleaved L R L R
free(mem);                              // викликач звільняє
```

Крейт **не алокує нічого й ніколи**. `harmonic_voice_process` пише
**interleaved-стерео** (`2·num_frames` семплів). Потокобезпеки немає —
не викликайте сеттери й `process` одночасно без зовнішньої синхронізації.

---

## 8. Міст до nih-plug

C-ABI у плагіні **не використовується**. `harmonic_synth` залежить від
`harmonic_core` як звичайний Rust path-крейт і викликає `PolySynth`
напряму.

`nih-plug` та `nih_plug_vizia` беруться з `harmonic_synth/vendor/nih-plug/` —
пропатчена копія pinned-дерева `de421011` (лише фікс CLAP `ext_state_load`,
`10_NIH_PLUG_CLAP_BUGS.md`), підключена через `[patch]` у
`harmonic_synth/Cargo.toml`. Прибрати після мержу фіксу upstream.

```rust
struct HarmonicSynth {
    params: Arc<HarmonicSynthParams>,   // + #[persist] editor_state: Arc<ViziaState>
    engine: PolySynth<24>,
    dly: [[f32;2]; HQ_LAT], dly_pos,    // PDC-компенсація коли HQ off
    analyzer: Box<SpectrumAnalyzer>,    // 30× band-pass Svf + 1 near-Nyquist BP (aliasing meter) + followers
    analyzer_bands: Arc<AnalyzerBands>, // [AtomicF32; 30] + alias_dbfs + voice_f0 + sample_rate + filter_cutoff + rolloff — audio→GUI, лок-free
    tuning_sig: Option<(i32,i32,i32,u64,u64)>, // (enum, root, ref×100, FNV Scala, FNV .kbm) — гейт ретюну в process
    mpe_timbre / poly_press: [f32; 128],// понотна експресія: MPE-тембр + поліафтертач на клавішу
// + params: #[persist] morph_a / morph_b: Mutex<Vec<(id, norm)>>, morph_pos: Mutex<f32> — A/B знімки
//           #[persist] seed: Mutex<u32> — останній seed рандомайзера (0 = немає)
    sustain_held: bool,                 // CC#64 стан педалі
    sustained_notes: [bool; 128],       // NoteOff, відкладені, поки педаль тримається
}

fn process(&mut self, buffer, _aux, context) -> ProcessStatus {
    // по-блоково: обгинаючі, FM-ratio, унісон, free-run, LFO, фільтр
    // подієвий цикл: NoteOn/Off/Choke/MidiPitchBend/CC#64 (sustain)/CC#123 (all notes off)
    //               + PolyBrightness/PolyPressure/MidiChannelPressure → понотна яскравість
    // посемплово: brightness, gain, character, feedback → render_sample() → [L,R]
    //             + analyzer.feed((L+R)/2)  лише якщо editor_state.is_open()
}
```

`MidiConfig::MidiCCs`, `SAMPLE_ACCURATE_AUTOMATION = true`, стерео-вихід
(`main_output_channels: NonZeroU32::new(2)`), 24 голоси (унісон ділить пул).

**GUI** (`src/editor.rs`, `nih_plug_vizia`): заголовок + спектр-дисплей
(`Spectrum` — власний `View`: 30 виміряних барів + гребінка партіалів
закритої форми + крива відгуку фільтра + метр аліасингу, щокадру) +
підпис + рядок пресетів + згруповані секції параметрів (TONE / AMP ENVELOPE / CHARACTER / FM / FILTER / VOICE / TUNING / MODULATION, `ParamSlider` + `ParamButton`) у `ScrollView`. Контент списку — в одному
`height: auto` VStack усередині `ScrollView` (як у `GenericUi` nih-plug), і
кожен `.group` / `.group-header` теж має явну `height: auto`: інакше morphorm
дає їм `Stretch(1.0)` і секції накладаються (див. `docs/11`, журнал REAPER).
Розмір вікна персиститься через `#[persist] editor_state`. Спектр-аналіз — не FFT, а банк
резонансних band-pass `Svf` (Q≈5, ⅓-октави) з envelope-фоловерами; результат
— 30 `AtomicF32`, які аудіо-потік пише, GUI читає.

**Гребінка закритої форми.** Поверх виміряних барів `Spectrum` малює
**реальні партіали** поточного патча: вага партіала `k` = `rᵏ + h·(aᵏ−bᵏ)`
(те саме, що `voice.rs::geom_osc`, з нормуванням на пік, яке для відносного
дисплея випадає), тонкі бурштинові вертикалі на `x_of(k·f0)`, плюс лінія на
стелі «Partials». `f0` — з `AnalyzerBands::voice_f0` (найнижча звучна нота,
`PolySynth::lowest_sounding_hz`, пишеться раз на блок коли редактор відкритий).
`r` — **живий**: `AnalyzerBands::rolloff` = `PolySynth::representative_rolloff()`
(ефективний `roll_eff` найнижчого голосу: згладжена яскравість + LFO→brightness
+ понотна експресія), тож нахил гребінки дихає разом із LFO; порожньо → фолбек
на `brightness_to_r(параметр)`. `partials` / `formant` — з параметрів напряму.
Лише `Geometric` (Saw/Triangle — фіксований `1/k`). «Математика на екрані,
поверх виміряного» — field-notes #2. Коли увімкнено унісон, кожен партіал
розмазується **на однакову ширину в пікселях** (стек голосів `±detune` центів
— стала в центах → стала в log-f; `Spectrum::unison_half_width_px`,
дзеркалить `poly.rs::note_on`) — бліда амбер-смуга за чіткою центральною
гребінкою.

**«Розбери цей пресет».** Наведення на слайдер Brightness / Partials / Formant
(`spectral_slider` → подія `HoverSpectral`, поле `Data::hovered_spectral`;
злиття `apply_hover` стійке до застарілого `leave`, що приходить після `enter`
наступного рядка) → `Spectrum::draw` домальовує 4 бліді криві-обгинаючі для
розгортки цього контролю — «ось що ця ручка робить зі спектром».
`Partials` бере `Param::preview_plain(s)` для своєї skewed-шкали.
`which` 0/1/2 (Brightness/Partials/Formant) розгортають гребінку партіалів;
3/4 (Cutoff/Resonance) — криву відгуку фільтра (бірюзова, тьмяніша; лише
коли `Filter ≠ Off`).

**Крива відгуку фільтра.** На тих самих осях `Spectrum` малює аналітичну
АЧХ рушійного `Svf` (бірюзова полілінія + вертикаль на частоті зрізу):
`Spectrum::filter_response` — це білінійно-предспотворений аналоговий
прототип SVF, який реалізує `harmonic_core::filter` (`g = tan(π f_c/f_s)`,
`k = 1/Q`, `Q = 0.5·2^{6·res}` — ті самі, що `Svf::recompute_{g,k}`; LP/BP/HP/
Notch зі спільного знаменника). Частота зрізу — **жива**: рушій пише
`AnalyzerBands::filter_cutoff` = `PolySynth::representative_cutoff()` (зріз
найнижчого звучного голосу зі згорнутими filter-envelope та LFO→cutoff) раз на
блок коли редактор відкритий; крива слідує за розгорткою в реальному часі, а
тьмяна півжирна вертикаль позначає **спокійне** положення ручки, щойно
модуляція його зсунула. Порожньо → фолбек на значення параметра. Малюється для
будь-якого осцилятора (Saw/Triangle теж фільтруються), тести
`editor::filter_response_curve_matches_the_svf_shape` +
`poly::representative_cutoff_tracks_the_filter_envelope`.

**Чесний метр аліасингу.** `Spectrum` малює праворуч окрему смугу — рівень
вузького band-pass на `0.44·f_s` (Q≈9) у dBFS, кольором за порогом
(зелений `< −45`, бурштин `−45…−30`, червоний `> −30`). Геометричний
осцилятор *точно* band-limited, тож енергія тут — це продукт нелінійних
стадій (Drive / Fold / Grit / FM / Feedback), що завернувся назад:
червоний ⇒ увімкни HQ Mode. Коли HQ увімкнено, майстер-дециматор цю
енергію знімає й метр падає — тобто він показує саме те, що HQ виправить.
`04_DSP_COMPONENTS.md §1.8`.

**A/B морфінг.** У редакторі — рядок `SET A · слайдер · SET B`. `SET A`/`SET B`
знімають нормалізовані значення **всіх** параметрів (`Params::param_map`) у
персистовані слоти. Слайдер пише `lerp(A, B, pos)` назад у справжні параметри
через `RawParamEvent` — тож хост бачить звичайні автоматизовані рухи ручок, а
звук точно відповідає ручкам. Фіксована архітектура (35 параметрів, без
модуляційної матриці) робить це коректним для кожного параметра; дискретні
(Oscillator / Filter / HQ) стрибають на середині. Це інструмент етапу дизайну
— діє лише поки редактор відкритий. `05 §3` / `12 §4` / `07 §18`.

**Seed-рандомайзер** (`src/rando.rs`) — той самий шлях запису параметрів.
`RANDOM` котить 30-бітний seed → патч; seed ↔ 6-символьний код (Crockford
base-32), мапінг seed→патч — цілочисельний SplitMix64, тож код звучить
однаково на будь-якій машині (`06 §6-bis`-детермінізм, тепер і для «випадкового»
звуку). `07 §19`.

Потокобезпека GUI→аудіо для параметрів — на `nih-plug`
(`FloatParam`/`EnumParam` lock-free).
