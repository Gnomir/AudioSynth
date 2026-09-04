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
| `poly` | ~450 | `PolySynth<VOICES>` |
| `ffi` | ~180 | C-ABI |

\* приблизно, включно з докстрінгами й тестами.

---

## 2. `Voice` — стан одного голосу

`#[repr(C)] #[derive(Clone, Copy)]`, **512 байт** (x86-64; `align = 8`), без
`Drop`, без вказівників. Розмір зростав із розвитком рушія — хост має
викликати `harmonic_voice_size()` у рантаймі, не хардкодити число.

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
n = clamp(⌊f_s / (2·f_eff)⌋, 1, 2048)                  [Найквіст-кламп по f_eff]
(geom-кеш: r^{n+1} та пік перераховуються лише коли (roll_eff, n) змінились)
  │
drift = drift_depth·sin_turns_fast(drift_phase++)     [повільний дрейф фази, якщо depth≠0]
pm = fm_index_eff·sin_turns(fm_phase) + feedback·last_osc + drift  [фазова модуляція]
osc = match waveform {                                 [Geometric — дефолт]
        Geometric => geometric_partials_pre(phase+pm, roll_eff, n, rn1) / peak
                     │  (rn1, peak із geom-кешу; hq → 2× оверсемпл + децимація)
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
    filter_mode, filter_cutoff, filter_res, filter_env,   // env_octaves
    fenv_a/d/s/r,                  // фільтрова ADSR
    unison_count, unison_detune, unison_spread, unison_drift,
    bend_ratio, lfo_rate, lfo_shape, lfo_mode,
    lfo_to_rolloff, lfo_to_pitch, lfo_to_cutoff, lfo_to_fm,
    counter: u64,                  // вік голосу для стилінгу
}

struct PolyVoice { core: Voice, amp: Adsr, filt_env: Adsr, note: u8, velocity: f32, age: u64 }
```

**Розподіл голосів** (`pick_voice`, у порядку пріоритету):
1. будь-який вільний (`!amp.is_active()`);
2. найстаріший у стадії release;
3. вкрасти глобально найстаріший.

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
- `panic = "abort"` у `[profile.release]`.

У плагіні ввімкнена дефолтна фіча `nih-plug` **`assert_process_allocs`** —
будь-яка алокація в `process()` панікує в хості (рантайм-контроль).

---

## 6. Матриця збірки

| Ціль | Команда | Що виходить |
|---|---|---|
| Розробка / тести | `cargo test` | `std` (дефолт), 70 тести (65 юніт + 5 інтеграційних) |
| Bit-exact на ARM | `harmonic_core/scripts/cross-verify.sh` | Docker + QEMU: `aarch64` + `armv7-hf`, 70/70, хеш = x86-64 |
| Приклади (WAV) | `cargo run --example <name> --release` | `*.wav` у теці крейта |
| **Справжній `no_std`** | `cargo build --no-default-features --release` | `cdylib` + `staticlib`, нуль `libc`-math, `panic=abort` |
| Явний SIMD | `cargo +nightly build --features portable-simd` | `#![feature(portable_simd)]` |
| Плагін | `cd harmonic_synth && cargo xtask bundle harmonic_synth --release` | `target/bundled/harmonic_synth.{vst3,clap}` |

`no_std`-збірка **тільки в `--release`** — dev-профіль потребує
`eh_personality` (unwind), а `panic=abort` заданий лише для релізу. Це
стандартна практика для embedded-крейтів.

---

## 7. Модель володіння FFI

`Voice` — POD, тому C-ABI не має `create`/`destroy`:

```c
size_t sz  = harmonic_voice_size();     // 512 сьогодні — НЕ хардкодити
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
    analyzer: Box<SpectrumAnalyzer>,    // 30× band-pass Svf + envelope followers
    analyzer_bands: Arc<AnalyzerBands>, // [AtomicF32; 30] — audio→GUI, lock-free
}

fn process(&mut self, buffer, _aux, context) -> ProcessStatus {
    // по-блоково: обгинаючі, FM-ratio, унісон, free-run, LFO, фільтр
    // подієвий цикл: NoteOn/Off/Choke/MidiPitchBend/CC#123
    // посемплово: brightness, gain, character, feedback → render_sample() → [L,R]
    //             + analyzer.feed((L+R)/2)  лише якщо editor_state.is_open()
}
```

`MidiConfig::Basic`, `SAMPLE_ACCURATE_AUTOMATION = true`, стерео-вихід
(`main_output_channels: NonZeroU32::new(2)`), 24 голоси (унісон ділить пул).

**GUI** (`src/editor.rs`, `nih_plug_vizia`): заголовок + спектр-дисплей
(`Spectrum` — власний `View`, малює 30 барів із `AnalyzerBands` щокадру) +
`GenericUi` у `ScrollView` (усі параметри). Розмір вікна персиститься через
`#[persist] editor_state`. Спектр-аналіз — не FFT, а банк резонансних
band-pass `Svf` (Q≈5, ⅓-октави) з envelope-фоловерами; результат — 30
`AtomicF32`, які аудіо-потік пише, GUI читає. Потокобезпека GUI→аудіо для
параметрів — на `nih-plug` (`FloatParam`/`EnumParam` lock-free).
