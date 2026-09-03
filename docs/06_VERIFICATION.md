# 06 — Верифікація

Що перевірено, як, і якими числами. Статус: **50 тестів проходять** (46
юніт + 4 інтеграційні), clippy чистий на трьох конфігураціях, плагін
збирається у VST3 + CLAP.

---

## 1. Методологія

| Клас перевірки | Метод |
|---|---|
| Коректність закритої форми | Порівняння з **прямою `Θ(n)` сумою** `Σ rᵏ cos(kx)` — незалежний еталон, не використовує основний конвеєр |
| Точність тригонометрії | Порівняння з `std`-математикою (`f64::{sin,cos,exp2,floor}`) на щільних розгортках |
| Форма АЧХ фільтра | RMS-відгук на синусоїду заданої частоти (однобіновий DFT / Goertzel) — перевірка смуг пропускання / загородження |
| Стійкість | Свіп-розгортки (cutoff × резонанс, кількість гармонік) на 10⁵–10⁶ семплів → відсутність NaN / вибуху |
| Обгинаючі | Монотонність стадій, вихід на sustain, звільнення голосу при sustain=0 |
| Стерео | Енергія різницевого сигналу `(L−R)²` vs сумарного `(L+R)²` — колапс до моно = нуль |
| Аліасинг | DFT рендереного голосу: енергія на кожній гармоніці до `n`, `< −80 дБ` вище клампу та в дзеркальних цілях |
| Вартість | Час рендеру на `n = 3` vs `n = 1200`; `Θ(n)` дав би `~400×` |
| RT-safety | `grep` не-тестового коду на `unwrap`/`expect`/`panic!`/алокації; `panic=abort`; `assert_process_allocs` у плагіні |

---

## 2. Каталог тестів

### `trig` (6)

| Тест | Що доводить |
|---|---|
| `cos_matches_reference_across_many_turns` | `cos_turns` vs `std`, макс. похибка `< 2·10⁻¹¹` абс. на `t ∈ [−37, 37]` |
| `sin_matches_reference_across_many_turns` | те саме для `sin_turns` |
| `fast_trig_is_16bit_accurate` | `cos/sin_turns_fast` vs `std`, макс. похибка `< 5·10⁻⁶` (для LFO/панорами) |
| `exp2_matches_reference` | `exp2` vs `std`, макс. відн. похибка `< 1.5·10⁻⁶` на `x ∈ [−60, 60]` |
| `floor_f64_matches_reference` | точна відповідність `f64::floor` |
| `exact_at_cardinal_points` | `cos_turns` у `{0, 0.25, 0.5, 2.5}` = `{1, 0, −1, −1}` |

### `lib::sr_tests` (1)

| Тест | Що доводить |
|---|---|
| `sample_rate_validation_reports_instead_of_substituting` | `validate_sample_rate` клампить (не підставляє 48k) і повертає `Ok`/`ClampedLow`/`ClampedHigh`/`Defaulted`; `Voice::new_checked` та `PolySynth::set_sample_rate` пробрасують статус |

### `kernel` (5)

| Тест | Що доводить |
|---|---|
| `dirichlet_matches_naive_sum` | `D_n` vs `Σ cos(kx)`, збіг `< 10⁻⁹·n` (`n` до 1024) |
| `geometric_matches_naive_sum` | `S_n` vs `Σ rᵏ cos(kx)` для `r ∈ {0.3…0.999}`, `n` до 1024 |
| `dirichlet_peak_and_dc` | пік = `n`, середнє за період `< 10⁻²` |
| `batched_x4_matches_scalar` | `geometric_partials_x4` полейнно = `geometric_partials`, `n` до 1500, `r` до 1.0 |
| `geometric_reduces_to_fundamental_for_small_r` | `r = 10⁻³` → нормований вихід ≈ `cos(x)` у межах `5·10⁻³` |

### `character` (5)

| Тест | Що доводить |
|---|---|
| `clean_params_are_bit_identity` | `CLEAN` → `process(x) == x` бітово, на 2000 значеннях |
| `drive_adds_energy_but_stays_bounded` | `drive 0.8` піднімає тихий сигнал (пік `> 0.3`), лишається `|y| ≤ 1.05` |
| `fold_and_grit_stay_finite_and_bounded` | усі 5 стадій разом на 48000 семплів → скінченне, `|y| ≤ 1.2` |
| `hq_path_is_bounded_and_reduces_alias_energy` | 2×+децимація на near-Nyquist тоні в фолдер → менше LF-енергії (аліасів), ніж 1× |
| `round_f32_behaves` | `round_f32` округлює до найближчого |

### `filter` (7)

| Тест | Що доводить |
|---|---|
| `bypass_is_identity` | `Bypass` → `process(x) == x` бітово |
| `lowpass_passes_low_blocks_high` | @1 kHz cutoff: 100 Hz RMS `> 0.5`, 10 kHz RMS `< 0.05` |
| `highpass_blocks_low_passes_high` | навпаки |
| `bandpass_peaks_near_cutoff` | @2 kHz: відгук на 2 kHz `> 3×` відгуку на 200 Hz та 16 kHz |
| `resonance_lifts_the_corner` | `res 1.0` → відгук на частоті зрізу `> 2×` проти `res 0` |
| `per_sample_smoothing_removes_the_zipper` | cutoff кидається 300↔8000 Hz щосемпла → макс. стрибок виходу `< 0.35` |
| `stable_under_cutoff_and_resonance_sweep` | свіп cutoff при `res 1.0`, 200 000 семплів, `|y| < 20`, скінченне |

### `env` (4)

| Тест | Що доводить |
|---|---|
| `ar_shape_when_sustain_is_full` | attack сягає 1, sustain тримає, release падає до `< 10⁻³` |
| `decays_to_sustain_and_holds` | decay осідає на `sustain = 0.4` у межах `0.02` |
| `zero_sustain_is_percussive_and_frees` | `sustain = 0` → голос стає Idle навіть при затиснутій ноті |
| `monotone_attack_then_nonincreasing_release` | attack монотонно росте, release монотонно спадає |

### `lfo` (3)

| Тест | Що доводить |
|---|---|
| `all_shapes_stay_in_range_and_have_zero_mean` | усі форми `∈ [−1,1]`, середнє за цикл `< 0.02` |
| `shapes_are_phase_aligned_at_start` | sine, triangle, saw усі `≈ 0` у фазі 0 |
| `triangle_and_saw_hit_their_peaks` | пік `> 0.95`, мін `< −0.95` |

### `voice` (6)

| Тест | Що доводить |
|---|---|
| `output_stays_bounded_across_the_range` | `f₀ ∈ {20…12000}`: стерео-пік `∈ (0.05, 1.5]`, скінченне |
| `blit_saw_and_triangle_are_bounded_and_shaped` | `Saw`/`Triangle` на `f₀ ∈ {55, 220, 3000}`: `\|y\| ≤ 1.6`, енергія над Найквістом `< 2 %·h₁`, гармоніки спадають; трикутник — парні `< 15 %`, `h₃/h₁ ∈ [0.06, 0.22]` (≈ `1/9`) |
| `equal_power_pan_splits_correctly` | hard-left «протікання» `< 5 %`; центр збалансований `< 5 %` |
| `free_running_phase_survives_note_on` | `free_running=true` → фаза не змінилась на `reset()`; `false` → фаза = 0 |
| `declick_ramps_in_from_near_zero` | перший семпл після `reset()` тихіший за пік перших 64 |
| `pitch_bend_and_lfo_stay_finite` | bend `+2 st` + LFO вібрато `25 ct` → пік `≤ 1.5` на 96000 семплів |

### `poly` (9)

| Тест | Що доводить |
|---|---|
| `midi_pitch_reference` | `midi_to_hz(69)=440`, `(60)≈261.63`, `(33)≈55` |
| `note_produces_bounded_sound_then_silence` | звук `> 0.05`, після note-off → 0 голосів, хвіст `< 10⁻⁴` |
| `voice_stealing_never_panics_or_clips` | 40 note-on на 4-голосний → `≤ 4` активних, `|L|,|R| ≤ 1.001` |
| `unison_stacks_voices_and_spreads_stereo` | `unison 4` → 4 голоси; ширина `(L−R)²/(L+R)² > 0.05`; після note-off → 0 |
| `pitch_bend_shifts_all_voices` | `unison 3` + bend `+2 st` / `−12 st` → пік `≤ 1.5` |
| `hq_mode_stays_bounded_and_adds_latency` | `set_hq(true)` + drive+fold → пік `≤ 1.5` на 48000 |
| `lfo_modulation_stays_bounded` | LFO triangle `→bright 0.35` + вібрато `30 ct` → пік `≤ 1.5` на 96000 |
| `filter_envelope_is_independent_of_amp_envelope` | фільтровий свіп (`sustain 0`) закриває HF `> 1.5×`, поки амплітудна ADSR тримає ноту |
| `soft_clip_is_gentle_and_bounded` | `≈` identity при `x ≤ 0.1`; `|soft_clip(±1000)| ≤ 1` |

### `tests/spectrum.rs` — інтеграційні (4)

| Тест | Що доводить |
|---|---|
| `closed_form_equals_bruteforce` | `D_n` vs пряма сума, `n` до 2048, `< 5·10⁻⁸·n + 10⁻⁶` |
| `geometric_is_a_true_finite_sum` | усічення на `n` vs на `4n` збігаються (`< 10⁻⁶`) → форма скінченна, не нескінченна |
| `rendered_voice_does_not_alias` | DFT рендеру @440 Hz, `r=0.995`: енергія на `f₀` та 10-й гармоніці присутня; `< 10⁻⁴` вище клампу (54 гарм.) та в дзеркальних цілях |
| `cost_is_flat_in_partial_count` | час(1200 гарм.) / час(3 гарм.) `< 25×` (не `~400×`) |

---

## 3. Виміряні числа

### Пропускна здатність (`examples/bench_hc.rs`, реліз, скаляр, без SIMD)

| f₀ | гармонік | семплів/с | × realtime @48k |
|---|---|---|---|
| 8000 Hz | 3 | ~21.9 M | ~456 |
| 880 Hz | 27 | ~20.8 M | ~433 |
| 110 Hz | 218 | ~20.6 M | ~430 |
| 20 Hz | 1200 | ~20.0 M | ~417 |

(після переводу LFO/панорами на швидку тригонометрію — ~+5 % vs попередній вимір)

Розкид ~10 % на діапазоні гармонік ×400 → вартість плоска по `n`
(підтверджує `Θ(log n)`, спростовує `Θ(n)`).

### Спектральний ефект character (`examples/character_demo.rs`)

Відношення енергії верх (4–18 kHz) / середина (150–1500 Hz):

| Стан | hi/lowmid |
|---|---|
| clean | 0.01 |
| + drive | 0.02 |
| + fold | 0.10 |
| + grit | 0.03 |
| FM свіп (index→3) | 0.78 |
| feedback свіп (→0.7) | 0.31 |

### Фільтр (`examples/filter_demo.rs`)

| Стан | hi/low |
|---|---|
| LP, cutoff ~200 Hz | 0.01 |
| LP, cutoff ~13 kHz | 1.21 |
| BP, cutoff ~2 kHz | 5.72 |
| фільтрова ADSR — одразу після щипка | HF slope 0.24 |
| — через 0.5 с (обгинаюча спала, нота звучить) | HF slope 0.02 (`~11×` падіння) |

### Унісон (`examples/wide_demo.rs`, 7× на ноту)

Ширина стерео `(L−R)²/(L+R)²` = **0.61**, пік = **0.98** (soft-clip тримає).

### Апаратура вимірювання

Споживчий ноутбук x86-64, Windows 11, Rust `stable-x86_64-pc-windows-msvc`
1.97, `[profile.release] opt-level=3, lto=true, codegen-units=1`.

---

## 4. Статус лінтингу / збірки

| Конфігурація | Команда | Результат |
|---|---|---|
| std, усі цілі | `cargo clippy --all-targets` | 0 попереджень / помилок |
| no_std реліз | `cargo clippy --no-default-features --release` | 0 |
| nightly SIMD | `cargo +nightly build --features portable-simd` | збирається |
| Тести | `cargo test` | 49 / 49 |
| no_std бінарник | `cargo build --no-default-features --release` | `harmonic_core.dll` (~14 КБ) + `.lib` |
| Плагін | `cargo xtask bundle harmonic_synth --release` | `.vst3` + `.clap`; `clap_entry` присутній, VST3 має `GetPluginFactory`/`InitDll`/`ExitDll` |

---

## 5. RT-safety — grep

```
$ grep -nE 'unwrap\(\)|expect\(|panic!' src/*.rs | grep -v '#\[cfg(test)\]' ...
```
→ збіги **лише** у `#[cfg(test)]`-модулях (`env.rs` тести). Нуль у гарячому
шляху.

---

## 6. Валідація плагіна — виконано

`cargo xtask validate` (або `harmonic_synth/scripts/validate.{ps1,sh}`) збирає
бандли й проганяє **pluginval на `.vst3`** та **clap-validator на `.clap`**.
Обидва покривають: state recall, зміну блоку/SR хостом на льоту, перевірку
алокацій у `process`, автоматизацію, потокобезпеку, fuzzing.

| Формат | Утиліта | Результат |
|---|---|---|
| VST3 | `pluginval --strictness-level 8 --validate-in-process` | **SUCCESS — повний прохід** (усі тести, включно з Plugin state / state restoration) |
| CLAP | `clap-validator` | **31 / 31** з виключеними `state-reproducibility-*` та `state-invalid-random` (9 skipped — N/A: GUI, note-ports тощо) |

**Виключені CLAP-тести — це баги nih-plug, не наші** (rev `de421011`,
`src/wrapper/clap/wrapper.rs`):
1. `ext_state_load` викликає `set_state_inner` напряму й **не постить
   `Task::RescanParamValues`** → хост не дізнається, що значення параметрів
   змінились після `load`. (VST3-шлях `pluginval` це проходить — стан
   круглить нормально.)
2. `ext_state_load`: `Vec::with_capacity(length as usize)` з невалідованим
   `length` зі стріму → alloc-abort (`0xc0000409`) на випадкових байтах
   (`state-invalid-random`).

Обидва підтверджені на `master`. Фікс написаний і перевірений локально
(`[patch]` на клон nih-plug → **`clap-validator` 35/35, 0 fail**); корінь,
репро та патч — **`10_NIH_PLUG_CLAP_BUGS.md`**. Публічний upstream-PR ще не
подано. Після мержу — прибрати `--exclude` зі скриптів.

Апаратура: Windows 11 x86-64, pluginval 1.0.4, clap-validator 0.4.1.

---

## 7. Що НЕ покрито

- **Живий DAW** (Ableton, Bitwig, Reaper, Logic) — не запускалось (лише
  pluginval / clap-validator, §6).
- **CLAP state round-trip** — блокований багами nih-plug (§6); VST3 state — ок.
- **Не-x86 таргети** (ARM Cortex-M, AArch64, RISC-V) — код `no_std`-сумісний
  і не має платформних припущень, але не компілювався/не тестувався там.
- **Частоти дискретизації поза `[8000, 768000]` Hz** — тепер клампляться зі
  статус-кодом (не тихо), але сам кламп-шлях у реальному хості не тестований.
- **Довготривалий числовий дрейф** фазового акумулятора (`f64` phase, wrap
  щоперіоду — накопичення похибки обмежене, але не виміряне на годинах).
- **Автоматизація параметрів на межі блоку** в реальному хості (тестовано
  лише логіку рушія, не marshalling `nih-plug`).
- **`geometric_partials_x4_simd`** на nightly перевірено лише що
  **компілюється** — числова еквівалентність скаляру не має окремого тесту
  (батч-версія `geometric_partials_x4` — має).
