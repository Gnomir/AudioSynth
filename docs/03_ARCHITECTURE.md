# 03 — Архітектура

Два крейти:

- **`harmonic_core`** — `#![no_std]`-сумісний DSP-рушій, нуль залежностей,
  C-ABI. Тут уся математика й синтез.
- **`harmonic_synth`** — плагін VST3 + CLAP над `harmonic_core`, через
  `nih-plug`. Лише хостовий клей: параметри, маршрутизація MIDI, семпловий
  цикл.

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
| `lfo` | ~130 | `Lfo` |
| `voice` | ~330 | `Voice` — повний тракт одного голосу, стерео-вихід |
| `poly` | ~430 | `PolySynth<VOICES>` |
| `ffi` | ~180 | C-ABI |

\* приблизно, включно з докстрінгами й тестами.

---

## 2. `Voice` — стан одного голосу

`#[repr(C)] #[derive(Clone, Copy)]`, ~200 байтів, без `Drop`, без вказівників.

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
    // --- старт ноти ---
    free_running: bool,              // true = фаза переживає note-on
    declick: u16,                    // семплів у fade-in, що лишились
    // --- FM ---
    fm_phase: f64, fm_ratio: f64, fm_index: f64,
    feedback: f64, last_osc: f64,
    // --- LFO ---
    lfo: Lfo,
    lfo_to_rolloff: f64,             // ± до r
    lfo_to_pitch: f64,               // центи вібрато
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
LFO.tick() → m ∈ [−1,1]
  ├─ pitch_mod = 2^(lfo_to_pitch · m / 1200)          [exp2, тільки якщо ≠ 0]
  ├─ f_eff     = freq_z · bend_z · pitch_mod
  └─ roll_eff  = clamp(rolloff_z + lfo_to_rolloff · m, 1e-3, 0.9995)
  │
n = clamp(⌊f_s / (2·f_eff)⌋, 1, 2048)                  [Найквіст-кламп по f_eff]
  │
pm = fm_index·sin_turns(fm_phase) + feedback·last_osc  [фазова модуляція]
osc = geometric_partials(phase + pm, roll_eff, n) / geometric_peak(roll_eff, n)
last_osc ← osc
  │
shaped   = character.process(osc)                      [identity якщо clean]
filtered = filter.process(shaped)                      [identity якщо Bypass; cutoff/res згладжено ВСЕРЕДИНІ]
  │
dg   = declick-рампа (16 семплів, 1/16 → 1)
mono = filtered · gain · dg
  │
advance fm_phase (+= f_eff·fm_ratio/f_s, wrap)
advance phase    (+= f_eff/f_s, wrap)
  │
(sin_p, cos_p) = sin_cos_turns((pan_z·0.5 + 0.5)·0.25) [equal-power]
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
    unison_count, unison_detune, unison_spread,
    bend_ratio, lfo_rate, lfo_shape, lfo_to_rolloff, lfo_to_pitch,
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
| Розробка / тести | `cargo test` | `std` (дефолт), 45 тестів |
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
size_t sz  = harmonic_voice_size();     // ≈ 200
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

```rust
struct HarmonicSynth { params: Arc<HarmonicSynthParams>, engine: PolySynth<24> }

fn process(&mut self, buffer, _aux, context) -> ProcessStatus {
    // по-блоково: обгинаючі, FM-ratio, унісон, free-run, LFO, фільтр
    // подієвий цикл: NoteOn/Off/Choke/MidiPitchBend/CC#123
    // посемплово: brightness, gain, character, feedback → render_sample() → [L,R]
}
```

`MidiConfig::Basic`, `SAMPLE_ACCURATE_AUTOMATION = true`, стерео-вихід
(`main_output_channels: NonZeroU32::new(2)`), 24 голоси (унісон ділить пул).
Потокобезпека GUI→аудіо — цілком на `nih-plug` (`FloatParam`/`EnumParam`
lock-free); власних атоміків немає.
