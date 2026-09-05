# 06 — Верифікація

Що перевірено, як, і якими числами. Статус: **70 тестів проходять** (65
юніт + 5 інтеграційних) + 1 `#[ignore]` (довготривалий дрейф, §3), clippy
чистий на трьох конфігураціях, плагін збирається у VST3 + CLAP, увесь набір
проходить біт-у-біт на `aarch64` + `armv7-hf` під QEMU (§6).

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
| Платформна ідентичність | хеш рендеру всього тракту звіряється біт-у-біт із x86-64 референсом на ARM під QEMU (`scripts/cross-verify.sh`) |

---

## 2. Каталог тестів

### `trig` (7)

| Тест | Що доводить |
|---|---|
| `cos_matches_reference_across_many_turns` | `cos_turns` vs `std`, макс. похибка `< 2·10⁻¹¹` абс. на `t ∈ [−37, 37]` |
| `sin_matches_reference_across_many_turns` | те саме для `sin_turns` |
| `fast_trig_is_16bit_accurate` | `cos/sin_turns_fast` vs `std`, макс. похибка `< 5·10⁻⁶` (для LFO/панорами) |
| `exp2_matches_reference` | `exp2` vs `std`, макс. відн. похибка **`< 5·10⁻⁸`** (Remez мінімакс) на `x ∈ [−60, 60]`; `exp2(k) == 2ᵏ` точно для `k ∈ [−20, 20]` |
| `tan_turns_fast_accurate_on_the_svf_domain` | `tan_turns_fast` vs `std` на `turns ∈ [10⁻⁵, 0.225]` (домен SVF): відн. похибка `< 2·10⁻⁷` |
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

### `character` (9)

| Тест | Що доводить |
|---|---|
| `clean_params_are_bit_identity` | `CLEAN` → `process(x) == x` бітово, на 2000 значеннях |
| `tanh_pade_joins_the_clamp_smoothly` | нахил `tanh_pade` одразу перед клампом `±3` `< 2·10⁻³`, одразу за ним `< 10⁻⁶` (немає зламу першої похідної, який давав кламп `±4`); жодного перельоту `±1` на `x ∈ [0, 50]` |
| `sample_and_hold_state_is_cleared_on_reset` | після навантаженого drive+downsample та `reset()` перший семпл на тишу = `0` бітово (S&H `hold` не тягне хвіст попередньої ноти) |
| `dc_blocker_time_constant_matches_between_1x_and_2x_paths` | загасання DC-зсуву через `process` (1×) і `process_hq_pair` (2×) збігається на матчнутих лічильниках викликів — доводить `√`-масштабування коефіцієнта DC-blocker'а на 2×-шляху (без цього падає на семплі 2000 з розбіжністю `0.263` проти `0.097`) |
| `tiny_downsample_is_bypassed_not_jittered` | `downsample = 9·10⁻⁵` (щойно під bypass-порогом `10⁻⁴`) бітово ідентичний `downsample = 0.0` на 5000 семплах (без порогу падає точно на семплі 741, де мав би спрацювати пропуск S&H-лічильника) |
| `drive_adds_energy_but_stays_bounded` | `drive 0.8` піднімає тихий сигнал (пік `> 0.3`), лишається `|y| ≤ 1.05` |
| `fold_and_grit_stay_finite_and_bounded` | усі 5 стадій разом на 48000 семплів → скінченне, `|y| ≤ 1.2` |
| `hq_path_is_bounded_and_reduces_alias_energy` | 2×+децимація на near-Nyquist тоні в фолдер → менше LF-енергії (аліасів), ніж 1× |
| `round_f32_behaves` | `round_f32` округлює до найближчого |

### `filter` (9)

| Тест | Що доводить |
|---|---|
| `bypass_is_identity` | `Bypass` → `process(x) == x` бітово |
| `lowpass_passes_low_blocks_high` | @1 kHz cutoff: 100 Hz RMS `> 0.5`, 10 kHz RMS `< 0.05` |
| `highpass_blocks_low_passes_high` | навпаки |
| `bandpass_peaks_near_cutoff` | @2 kHz: відгук на 2 kHz `> 3×` відгуку на 200 Hz та 16 kHz |
| `resonance_lifts_the_corner` | `res 1.0` → відгук на частоті зрізу `> 2×` проти `res 0` |
| `per_sample_smoothing_removes_the_zipper` | cutoff кидається 300↔8000 Hz щосемпла → макс. стрибок виходу `< 0.35` |
| `set_sample_rate_retargets_the_prewarp_and_the_clamp` | подвоєння `sample_rate` при тому самому Hz cutoff → `g` зменшується `2×` (прямий доказ, що `recompute_g` перерахувався на нову ставку); повторний кламп у `set_sample_rate` no-op при тій самій ставці |
| `cutoff_ceiling_is_identical_at_1x_and_hq_2x` | той самий запитаний cutoff (до `500 000` Гц) клампується **однаково** при `Svf::new(fs)` і після `set_sample_rate(2fs)` — доводить, що музична стеля прив'язана до `base_sample_rate`, не до робочої ставки (без цього HQ відкривав би фільтр удвічі далі за той самий свіп) |
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

### `poly` (14)

| Тест | Що доводить |
|---|---|
| `midi_pitch_reference` | `midi_to_hz(69)=440`, `(60)≈261.63`, `(33)≈55` |
| `soft_clip_joins_the_clamp_smoothly` | `poly::soft_clip` — незалежна текстова копія `character::tanh_pade` — має ту саму C²-гладкість на клампі `±3`, ту саму відсутність перельоту; регресія на випадок, якщо копії розійдуться |
| `clamps_reject_nan_instead_of_latching_it` | `NaN`-гейн + `NaN`-velocity на `PolySynth` → скінченний вихід на 4800 семплах, не NaN назавжди (`NaN < x`/`NaN > x` завжди `false`, тож голий `if`-кламп пропускав би `NaN` без змін) |
| `hq_bus_master_clip_stays_under_75db_alias_floor` | HQ-шина, майстер вбитий `soft_clip`-ом (~6 дБ перевантаження); енергія на НЕ-гармонічних пробних частотах `≤ −75` дБ від фундаменталу (виміряно `−94.6` дБ). Захват 2²⁰ семплів без вікна — інакше витік бічних пелюстків сильної гармоніки маскується під «аліасинг» |
| `unison_drift_makes_the_image_breathe` | детюн `0` → віконна ширина `side/(mid+side)` нерухома (span `< 0.06`); `drift 0.7` → span `> 3×` більший і `> 0.05`; образ не колапсує (§3) |
| `note_produces_bounded_sound_then_silence` | звук `> 0.05`, після note-off → 0 голосів, хвіст `< 10⁻⁴` |
| `voice_stealing_never_panics_or_clips` | 40 note-on на 4-голосний → `≤ 4` активних, `|L|,|R| ≤ 1.001` |
| `unison_stacks_voices_and_spreads_stereo` | `unison 4` → 4 голоси; ширина `(L−R)²/(L+R)² > 0.05`; після note-off → 0 |
| `pitch_bend_shifts_all_voices` | `unison 3` + bend `+2 st` / `−12 st` → пік `≤ 1.5` |
| `hq_mode_stays_bounded_and_adds_latency` | `set_hq(true)` + drive+fold → пік `≤ 1.5` на 48000 |
| `lfo_modulation_stays_bounded` | LFO triangle `→bright 0.35` + вібрато `30 ct` → пік `≤ 1.5` на 96000 |
| `filter_envelope_is_independent_of_amp_envelope` | фільтровий свіп (`sustain 0`) закриває HF `> 1.5×`, поки амплітудна ADSR тримає ноту |
| `extreme_cutoff_modulation_never_destabilises_the_filter` | база 12 кГц + LFO→cutoff на клампі `±8` окт + envelope `+6` окт + res `0.02` (найбільше `k`, найнебезпечніший режим для полюса `tan_turns_fast` на `0.25`) — скінченне, `< 20` на 48000 семплів |
| `soft_clip_is_gentle_and_bounded` | `≈` identity при `x ≤ 0.1`; `|soft_clip(±1000)| ≤ 1` |

### `tests/spectrum.rs` — інтеграційні (4)

| Тест | Що доводить |
|---|---|
| `closed_form_equals_bruteforce` | `D_n` vs пряма сума, `n` до 2048, `< 5·10⁻⁸·n + 10⁻⁶` |
| `geometric_is_a_true_finite_sum` | усічення на `n` vs на `4n` збігаються (`< 10⁻⁶`) → форма скінченна, не нескінченна |
| `rendered_voice_does_not_alias` | DFT рендеру @440 Hz, `r=0.995`: енергія на `f₀` та 10-й гармоніці присутня; `< 10⁻⁴` вище клампу (54 гарм.) та в дзеркальних цілях |
| `cost_is_flat_in_partial_count` | час(1200 гарм.) / час(3 гарм.) `< 25×` (не `~400×`) |

### `tests/cross_platform_bit_exact.rs` — інтеграційний (1)

| Тест | Що доводить |
|---|---|
| `rendered_signal_is_bit_identical_across_architectures` | 100 мс рендеру `PolySynth<8>` через весь тракт (унісон 4 + drift + FM + feedback + 4 маршрути LFO + резонансний Low SVF + drive/bias/fold/crush/downsample), зі скриптованими note-on/off та pitch-bend; біти кожного семпла згортаються в FNV-1a хеш і звіряються з константою, знятою на `x86_64-pc-windows-msvc`. Будь-яка розбіжність в 1 ULP на ~9600 семплах змінює хеш. Зелений на x86-64, `aarch64-unknown-linux-gnu`, `armv7-unknown-linux-gnueabihf` (§6) |

---

## 3. Виміряні числа

### Пропускна здатність — один голос (`examples/bench_hc.rs`, реліз, скаляр)

Чистий голос (character CLEAN, filter Bypass, LFO не роутований) — **clean
fast path**: LFO не тикається, equal-power гейни та `powi_pos(r, n±1)`
кешуються на сталій ноті.

| f₀ | гармонік | семплів/с | × realtime @48k |
|---|---|---|---|
| 8000 Hz | 3 | ~26.4 M | ~550 |
| 880 Hz | 27 | ~26.3 M | ~548 |
| 110 Hz | 218 | ~26.5 M | ~552 |
| 20 Hz | 1200 | ~26.6 M | ~554 |

**Повністю плоско** 3↔1200 гармонік (розкид `< 1 %`) — кеш `powi_pos(r, n±1)`
на сталій ноті прибирає залишковий `Θ(log n)`.

### Пропускна здатність — поліфонія (`examples/bench_poly.rs`, `PolySynth<64>`)

Акорд на всі 64 голоси, ноти 24–94 (багато низьких → великий `n`):
**`~0.45 M` стерео-фрейм/с** = `~9.4×` realtime @48k = `~590` голосів у
realtime-запасі. Clean-voice fast path (кеш `powi_pos` + пан-гейни, LFO не
тикається нероутований) дає тут `~+80 %` — найбільше на низьких нотах із
сотнями гармонік.

### PolyBLEP пилка / трикутник

| | пропускна (M семпл/с) | фундаментал vs ідеал | `h₃/h₁` | alias-floor (`f₀ ≤ 1 кГц`) |
|---|---|---|---|---|
| Saw (PolyBLEP) | **~90** | `±0.02` дБ до 27.5 Гц | `0.333` (`1/3`) | `< −90` дБ |
| Triangle (PolyBLAMP) | **~77** | `±0.00` дБ до 27.5 Гц | `0.110` (`1/9`) | `< −95` дБ |

Дешевше за геометричну несучу (`~26 M`). Спад на високих `f₀` (`−0.11` дБ на
3 кГц, `−0.45` дБ на 6 кГц) — межа поліноміальної апроксимації; нечутно в
музиці. Без стану (`reset()` не чіпає).

### Тригонометричні ядра

| Ядро | Точність | Вартість |
|---|---|---|
| `exp2` (`2^f`, `f ∈ [0,1]`) | Remez мінімакс степені 7, макс. відн. похибка `2.2·10⁻⁸` (`~3·10⁻⁵` цента — за межею вимірності) | 8 членів Горнера |
| `tan_turns_fast` (прогин SVF) | `[3/2]` рац. мінімакс на `[0, 0.23]`, `< 10⁻⁷` | `~4×` менше флопів за `sin_turns/cos_turns`; повністю модульований `Svf` — `41 → 87 M` семпл/с (`2.1×`; решта — сам 2-полюсний TPT-крок і згладжування коефіцієнтів, не `tan`) |

### Unified HQ Bus

Кожен голос у HQ-режимі віддає недецимовану пару `2×`-семплів
(`Voice::render_hq_subsamples`, `pub(crate)`); `PolySynth` сумує всі голоси на
`2×`, майстер-сатурує обидва підсемпли, і децимує **рівно один раз** —
65-тапним лінійно-фазовим half-band FIR (17 унікальних коефіцієнтів,
Kaiser-вікно). Стандалон `Voice`/C-ABI — незалежний по-голосний шлях
(`Character::process_hq`, свій 13-тапний дециматор).

| Метрика | Значення |
|---|---|
| Аліасинг майстра (`+6` дБ вхід) | ціль `< −75` дБ, **виміряно `−94.6` дБ** (`poly::hq_bus_master_clip_stays_under_75db_alias_floor`, 2²⁰-семпловий захват) |
| Стопбенд майстер-дециматора | `−80` дБ на `1.166×` вихідного Найквіста (27-тапний half-band не може ближче ніж `1.7×`; 65 тапів практично безкоштовні, бо вартість не залежить від кількості голосів) |
| Латентність | `32` семпли на `2×` = **`16` семплів на `1×`, точно** (`PolySynth::HQ_LATENCY`; стандалон-шлях — `Voice::HQ_LATENCY = 3`) |
| CPU при HQ, 12 / 16 / 24 голоси | `+0.8…+2.5 %` / `+2.4…+2.7 %` / `+0.5…+1.4 %` — статистичний нуль на будь-якій поліфонії (`examples/bench_hq_bus.rs`) |

**CPU не зменшується** — по-голосний дециматор (13 тапів, 4 унікальні
коефіцієнти) ніколи не був домінантною вартістю: осцилятор + 5 стадій
`Character` на `2×` коштують на порядки більше, і нова архітектура додає
другий прохід `Svf` (раніше `1×` після децимації, тепер `2×` до неї), що
майже точно компенсує виграш від об'єднання дециматорів. Архітектурна мета
(один дециматор замість каскаду) і аліасинг-ціль (`−94.6` проти `−75` дБ)
досягнуті; CPU-виграш — ні.

### DC-blocker на `2×` + S&H bypass-поріг

| Механізм | Деталь |
|---|---|
| DC-blocker коефіцієнт на 2×-шляху | `0.999_749_97 = √0.9995` (з `R = exp(−2π·fc/f_op)`) — тримає `fc` фіксованим (`3.82` Гц) в обох шляхах; голий `0.9995` подвоїв би `fc` на `2×` |
| S&H bypass | поріг `downsample > 10⁻⁴` (не `> 0.0`): на мікроскопічному `downsample` дискретний hold/skip-лічильник давав періодичний `~700` Гц «хіккап» замість майже-тиші |

Обидва стосуються обох HQ-шляхів (`stage(x, 2.0)` по-голосний і `PolySynth`-
шина). Регресії (`character::{dc_blocker_time_constant_matches_between_1x_and_2x
_paths, tiny_downsample_is_bypassed_not_jittered}`) підтверджено ловити
конкретні розбіжності — тимчасовий відкат → передбачувана невдача на
конкретному семплі.

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

Образ модулюється в часі до `±11 %` ширини й не колапсує.

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
| Тести | `cargo test` | 70 / 70 (65 юніт + 5 інтеграційних) |
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

CLAP `35/35` вимагає локального `[patch]` на `harmonic_synth/vendor/nih-plug/`
— пропатченої копії pinned-дерева, що усуває два баги `ext_state_load`
CLAP-обгортки nih-plug (немає `rescan(CLAP_PARAM_RESCAN_VALUES)` після `load`;
`Vec::with_capacity` на невалідованій довжині зі стріму → alloc-abort на
пошкодженому пресеті). Корінь, патч, склад vendor, коли прибрати —
**`10_NIH_PLUG_CLAP_BUGS.md`**; публічний upstream-PR — за користувачем
(`09`).

Апаратура: Windows 11 x86-64, pluginval 1.0.4, clap-validator 0.4.1.

---

## 6-bis. Платформна бітова ідентичність — виконано

`scripts/cross-verify.{sh,ps1}` (Docker + QEMU, `rust:1.97-slim`) проганяє
**весь набір `harmonic_core`** на емульованих ARM-таргетах:

| Таргет | `f64`-FPU | Результат |
|---|---|---|
| `aarch64-unknown-linux-gnu` | AdvSIMD/FP | **70 / 70 pass** (65 юніт + 5 інтеграційних) |
| `armv7-unknown-linux-gnueabihf` | VFPv3-d16 — **тотожний Cortex-M4F** | **62 / 62 pass** |

`rendered_signal_is_bit_identical_across_architectures` звіряє хеш 100-мс
рендеру всього тракту з референсом, знятим на `x86_64-pc-windows-msvc`:
**дельта `= 0.0` на всіх трьох архітектурах**. Дрейф фазового акумулятора
(`voice::phase_accumulators_do_not_drift`, `DRIFT_SAMPLES=2·10⁷`) теж
збігається до останньої значущої цифри (`4.566·10⁻¹⁰` обертів carrier,
`3.483·10⁻¹⁰` FM) на x86-64 та `aarch64`.

Обґрунтування: жоден гарячий шлях не використовує `libm`, `mul_add` чи FMA-
контракцію; x86-64 на SSE2 (без x87 80-біт); касти `(x as i64)` насичувані
на рівні мови. IEEE-754 `+ − × ÷` коректно округлені однаково на SSE2 та
VFP/NEON → результат мусить збігатися, і тепер це **виміряно**, не виведено.

Ще ні: реальне залізо Cortex-M, `thumbv6m` (M0 без FPU — програмний `f64`),
прогін під RISC-V (усі три крос-компілюються чисто).

Апаратура: Docker Desktop 29.7, QEMU user-mode через `binfmt_misc`.

---

## 7. Що НЕ покрито

- **Живий DAW** (Ableton, Bitwig, Reaper, Logic) — не запускалось (лише
  pluginval / clap-validator, §6).
- **Регресійний тест на CLAP `ext_state_load`-фікс** — сам фікс перевіряється
  лише `clap-validator` (у `cargo xtask validate`, не в `cargo test`).
- **ARM під QEMU — покрито** (§6-bis: `aarch64` + `armv7-hf`, 70/70, хеш
  біт-у-біт). **Не покрито:** реальне залізо Cortex-M, `thumbv6m` (M0,
  soft-float `f64`), прогін під RISC-V — усе крос-компілюється чисто, але не
  проганялось.
- **Частоти дискретизації поза `[8000, 768000]` Hz** — тепер клампляться зі
  статус-кодом (не тихо), але сам кламп-шлях у реальному хості не тестований.
- **Автоматизація параметрів на межі блоку** в реальному хості (тестовано
  лише логіку рушія, не marshalling `nih-plug`).
- **`geometric_partials_x4_simd`** на nightly перевірено лише що
  **компілюється** — числова еквівалентність скаляру не має окремого тесту
  (батч-версія `geometric_partials_x4` — має).
