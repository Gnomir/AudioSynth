# 06 — Верифікація

Що перевірено, як, і якими числами. Статус: **58 тестів проходять** (54
юніт + 4 інтеграційні) + 1 `#[ignore]` (довготривалий дрейф, §3), clippy
чистий на трьох конфігураціях, плагін збирається у VST3 + CLAP.

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

### `kernel` (6)

| Тест | Що доводить |
|---|---|
| `dirichlet_matches_naive_sum` | `D_n` vs `Σ cos(kx)`, збіг `< 10⁻⁹·n` (`n` до 1024) |
| `pre_variants_are_bit_identical` | `geometric_partials_pre` / `geometric_peak_pre` == оригінали **бітово** для `powi_pos(r, n±1)` (гарантія кешу fast path) |
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

### `lfo` (4)

| Тест | Що доводить |
|---|---|
| `all_shapes_stay_in_range_and_have_zero_mean` | усі форми `∈ [−1,1]`, середнє за цикл `< 0.02` |
| `shapes_are_phase_aligned_at_start` | sine, triangle, saw усі `≈ 0` у фазі 0 |
| `triangle_and_saw_hit_their_peaks` | пік `> 0.95`, мін `< −0.95` |
| `free_run_mode_survives_retrigger` | `FreeRun` — `retrigger()` не чіпає фазу; `Retrigger` (дефолт) — скидає в 0 |

### `voice` (11 + 1 `#[ignore]`)

| Тест | Що доводить |
|---|---|
| `output_stays_bounded_across_the_range` | `f₀ ∈ {20…12000}`: стерео-пік `∈ (0.05, 1.5]`, скінченне |
| `polyblep_saw_and_triangle_are_bounded_and_shaped` | `Saw`/`Triangle` на `f₀ ∈ {55, 220, 3000}`: `\|y\| ≤ 1.6`, енергія над Найквістом `< 2 %·h₁`, гармоніки спадають; трикутник — парні `< 15 %`, `h₃/h₁ ∈ [0.06, 0.22]` (≈ `1/9`) |
| `polyblep_waves_are_flat_into_the_sub_bass` | пилка/трикутник на `f₀ ∈ {27.5, 55, 220}` Гц: фундаментал `±0.5` дБ від ідеального рівня (`2/π` / `8/π²`) — доводить відсутність HPF, який leaky-BLIT давав як `−5` дБ на 28 Гц |
| `unrouted_lfo_does_not_affect_output` | голос з LFO на якійсь частоті, але routing `= 0`, рендериться **бітово** так само, як без LFO (fast path не тикає LFO) |
| `lfo_to_cutoff_and_fm_stay_bounded` | усі 4 цілі роутингу разом на filtered+FM голосі → скінченне, `\|y\| ≤ 2.5` (резонансний SVF на швидкому свіпі перевищує unity — це реально) |
| `free_run_lfo_phase_survives_note_on` | `FreeRun` vs `Retrigger` голос після note-on посеред циклу LFO дають **різний** вихід (FreeRun не рестартує вібрато) |
| `geom_and_pan_caches_track_changing_params` | після зсуву `rolloff` + `pan` голос сходиться (`< 1e-4`) до значень свіжого голосу, стартованого прямо на цих параметрах → кеші інвалідуються коректно |
| `equal_power_pan_splits_correctly` | hard-left «протікання» `< 5 %`; центр збалансований `< 5 %` |
| `free_running_phase_survives_note_on` | `free_running=true` → фаза не змінилась на `reset()`; `false` → фаза = 0 |
| `declick_ramps_in_from_near_zero` | перший семпл після `reset()` тихіший за пік перших 64 |
| `pitch_bend_and_lfo_stay_finite` | bend `+2 st` + LFO вібрато `25 ct` → пік `≤ 1.5` на 96000 семплів |
| `phase_accumulators_do_not_drift` `#[ignore]` | `10⁹` семплів vs Kahan-еталон: похибка частоти несучої `< 10⁻³` ppm (виміряно `5·10⁻⁹`), FM так само (§3) |

### `poly` (10)

| Тест | Що доводить |
|---|---|
| `midi_pitch_reference` | `midi_to_hz(69)=440`, `(60)≈261.63`, `(33)≈55` |
| `unison_drift_makes_the_image_breathe` | детюн `0` → віконна ширина `side/(mid+side)` нерухома (span `< 0.06`); `drift 0.7` → span `> 3×` більший і `> 0.05`; образ не колапсує (§3) |
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

### Пропускна здатність — один голос (`examples/bench_hc.rs`, реліз, скаляр)

Чистий голос (character CLEAN, filter Bypass, LFO не роутований) — **clean
fast path**: LFO не тикається, equal-power гейни та `powi_pos(r, n±1)`
кешуються на сталій ноті.

| f₀ | гармонік | семплів/с | × realtime @48k | до fast path |
|---|---|---|---|---|
| 8000 Hz | 3 | ~26.4 M | ~550 | ~22.5 M |
| 880 Hz | 27 | ~26.3 M | ~548 | ~21.4 M (+23 %) |
| 110 Hz | 218 | ~26.5 M | ~552 | ~21.2 M (+25 %) |
| 20 Hz | 1200 | ~26.6 M | ~554 | ~21.1 M (+26 %) |

Тепер **повністю** плоско 3↔1200 гармонік (кеш `powi_pos` прибрав залишковий
`Θ(log n)`). Розкид `< 1 %`.

### Пропускна здатність — поліфонія (`examples/bench_poly.rs`, `PolySynth<64>`)

Акорд на всі 64 голоси, ноти 24–94 (багато низьких → великий `n`):

| | стерео-фрейм/с | × realtime @48k | ~голосів @ realtime |
|---|---|---|---|
| до fast path | ~0.25 M | ~5.1 | ~330 |
| **після** | **~0.45 M** | **~9.4** | **~590** |

**≈ +80 %** — виграш найбільший саме там, де боляче: низькі ноти з сотнями
гармонік раніше платили `powi_pos` щосемпла на кожен голос.

### PolyBLEP пилка / трикутник (заміна leaky-integrated BLIT)

| | пропускна (M семпл/с) | фундаментал vs ідеал | `h₃/h₁` | alias-floor (`f₀ ≤ 1 кГц`) |
|---|---|---|---|---|
| Saw (PolyBLEP) | **~90** | `±0.02` дБ до 27.5 Гц | `0.333` (`1/3`) | `< −90` дБ |
| Triangle (PolyBLAMP) | **~77** | `±0.00` дБ до 27.5 Гц | `0.110` (`1/9`) | `< −95` дБ |
| _(було: leaky-BLIT triangle)_ | ~25 | **`−5` дБ на 28 Гц** | 0.11 | `< −34` дБ (h₃ завищений) |

Спад на високих `f₀` (`−0.11` дБ на 3 кГц, `−0.45` дБ на 6 кГц) — межа
поліноміальної апроксимації; нечутно в музиці. `Voice` зменшився `576 → 512` б
(мінус 8 полів стану інтеграторів/DC-blockerів).

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

### Дрейф унісону — «дихання» (`poly::unison_drift_makes_the_image_breathe`)

6× унісон, детюн `0` (щоб базова ширина була нерухома), віконна ширина
`side/(mid+side)` за 20 вікон × 0.5 с:

| `drift` | span ширини між вікнами | середня ширина |
|---|---|---|
| `0.0` | `0.000` (ідеально статично) | `0.94` |
| `0.3` | `0.058` | `0.91` |
| `0.7` | `0.146` | `0.82` |
| `1.0` | `0.221` | `0.74` |

Образ модулюється в часі до `±11 %` ширини й не колапсує — DoD `09 §А3`.

### Довготривалий числовий дрейф (`voice::phase_accumulators_do_not_drift`, `#[ignore]`)

`10⁹` семплів безперервного рендеру — **5.79 год** аудіо @ 48 кГц —
проти Kahan-компенсованого точно-wrapped еталона:

| Акумулятор | Макс. відхилення фази | Еквів. похибка частоти |
|---|---|---|
| несуча (`phase += step`, wrap `−= 1` щоперіоду) | `2.28·10⁻⁸` обертів | **`5.0·10⁻⁹` ppm** |
| FM (`fm_phase`, wrap через `floor_f64`) | `1.74·10⁻⁸` обертів | `1.3·10⁻⁹` ppm |

Відхилення фази росте **лінійно** з `N` (систематичний bias округлення
`≈ 0.1 ulp/семпл`), але похибка *частоти* від `N` не залежить і становить
`~10⁻¹²` Гц на 220 Гц — на дев'ять порядків нижче за 1 цент. Підтверджує
аналіз `07_LIMITATIONS §1` / `08 §3`: wrap щоперіоду тримає `O(N·ε)`
обмеженим на практиці.

Прогін: `DRIFT_SAMPLES=1000000000 cargo test --release -- --ignored --nocapture drift`
(за замовч. `2·10⁸` = 1.16 год, ~11 с).

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
| VST3 | `pluginval --strictness-level 8 --validate-in-process` | **SUCCESS — повний прохід**, включно з `Editor` / `Open editor whilst processing` / `Editor Automation` (редактор `nih_plug_vizia`) та Plugin state / state restoration |
| CLAP | `clap-validator` (без `--exclude`) | **35 / 35, 0 failed, 0 warnings** (9 skipped — N/A: note-ports тощо) |

Раніше 4 тести стану (`state-reproducibility-{basic,binary,buffered}` FAIL,
`state-invalid-random` — жорсткий alloc-abort `0xc0000409`) блокувалися двома
багами `ext_state_load` у CLAP-обгортці nih-plug (`de421011` і `master`):
відсутній `rescan(CLAP_PARAM_RESCAN_VALUES)` після `load` та
`Vec::with_capacity` на невалідованій довжині зі стріму. **Виправлено** через
`[patch]` на `harmonic_synth/vendor/nih-plug/` (пропатчена копія pinned-дерева);
корінь, патч, склад vendor — **`10_NIH_PLUG_CLAP_BUGS.md`**. Публічний
upstream-PR — за користувачем.

Апаратура: Windows 11 x86-64, pluginval 1.0.4, clap-validator 0.4.1.

---

## 7. Що НЕ покрито

- **Живий DAW** (Ableton, Bitwig, Reaper, Logic) — не запускалось (лише
  pluginval / clap-validator, §6).
- **Регресійний тест на CLAP `ext_state_load`-фікс** — сам фікс перевіряється
  лише `clap-validator` (у `cargo xtask validate`, не в `cargo test`).
- **Не-x86 таргети** (ARM Cortex-M4F/M0, AArch64, RISC-V) — `no_std`-збірка
  **компілюється чисто** під усі чотири (`cargo build --no-default-features
  --release --target …`); прогін тестів на реальному залізі / QEMU — ні.
- **Частоти дискретизації поза `[8000, 768000]` Hz** — тепер клампляться зі
  статус-кодом (не тихо), але сам кламп-шлях у реальному хості не тестований.
- **Автоматизація параметрів на межі блоку** в реальному хості (тестовано
  лише логіку рушія, не marshalling `nih-plug`).
- **`geometric_partials_x4_simd`** на nightly перевірено лише що
  **компілюється** — числова еквівалентність скаляру не має окремого тесту
  (батч-версія `geometric_partials_x4` — має).
