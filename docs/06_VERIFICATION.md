# 06 — Верифікація

Що перевірено, як, і якими числами. Статус: **114 тестів `harmonic_core`** (96 юніт + 18 інтеграційних, з них 11 — ворожий RT-safety набір
`tests/stress.rs`) + **31 у плагіні** + **9 у `harmonic_license`** (analyzer, A/B morph, seed randomiser, preset bank, tuning, Scala `.scl` + `.kbm` import, спектр-гребінка + hover + крива фільтра + гребінка унісону, ліцензійний keyfile) + 1 `#[ignore]`
(довготривалий дрейф, §3). Clippy чистий на трьох конфігураціях, плагін
збирається у VST3 + CLAP, увесь набір ядра проходить біт-у-біт на `aarch64`
+ `armv7-hf` під QEMU (§6).

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

### `tuning` (11 + 1 у `poly`)

| Тест | Що доводить |
|---|---|
| `equal_440_is_bit_identical_to_midi_to_hz` | `Tuning::EQUAL_440.hz(n)` **побітово** (`to_bits()`) дорівнює `midi_to_hz(n)` для всіх 128 нот — доказ, що дефолтний шлях `note_hz` (короткий обхід на `midi_to_hz`) нічого не змінює. Разом із `is_equal_440()` — гарантія, що весь наявний крос-платформний хеш і всі тести лишаються зеленими |
| `equal_edo_matches_the_definition` | `Tuning::equal(12,…)` теж лягає на 12-TET; 24-EDO: одна MIDI-клавіша = чверть тону (`hz(70) = 440·2^{50/1200}`), 24 кроки = октава |
| `just_intonation_puts_the_fifth_at_a_pure_3_2` | 5-limit хроматична шкала від C4: тоніка не рухається, квінта (ступінь 7) — точно `3:2`, велика терція (ступінь 4) — точно `5:4`, октава подвоюється |
| `bohlen_pierce_repeats_at_the_tritave` | 13 рівних кроків «тритави» `1901.955` ц: 13 клавіш угору = точно `×3` |
| `hostile_scales_still_produce_finite_positive_frequencies` | `from_cents` із `NaN`/`±∞`/`±1e9` центами, `NaN` періодом і `NaN` ref-Hz → усі 128 нот дають скінченну додатну частоту (ref → 440, період → 1 ц) |
| `reference_frequency_scales_the_whole_scale` | `Tuning::equal(12, 432.0, 69)` → A4 = 432, а решта нот — 12-TET × `432/440` |
| `identity_kbm_is_bit_identical_to_the_linear_mapping` | явна тотожня `.kbm`-мапа над 12-EDO → `hz(n).to_bits()` **точно** дорівнює `equal(12,…)` на всіх 128 клавішах: `from_kbm` на дефолтному патерні недоторканий побайтово |
| `kbm_folds_a_seven_key_pattern_into_the_octave` | 7-клавішний патерн `[0,2,4,5,7,9,11]` над хроматикою: 7 клавіш угору = точно октава, клавіша +1 = ступінь 2 |
| `kbm_dead_keys_report_unmapped` | патерн із `-1` через клавішу: `is_mapped` = `false` на мертвих, `true` на живих; лінійна шкала мапить усе |
| `hostile_kbm_still_produces_finite_positive_frequencies` | `from_kbm` зі ступенями поза шкалою, величезними від'ємними, `NaN` формальною октавою і `∞` ref-Hz → усі 128 нот скінченні й додатні |
| `a_kbm_tuning_is_not_mistaken_for_the_default_fast_path` | нетотожній `keymap` → `is_equal_440()` = `false` (вимикає короткий обхід `midi_to_hz`) |
| `poly::a_dead_key_under_a_kbm_map_sounds_nothing` | `PolySynth` під `.kbm` із мертвими клавішами: `note_on(мертва)` — no-op (синт лишається тихим), жива клавіша грає, `set_tuning_equal` повертає все у мапу |

### `kernel` (8)

| Тест | Що доводить |
|---|---|
| `dirichlet_matches_naive_sum` | `D_n` vs `Σ cos(kx)`, збіг `< 10⁻⁹·n` (`n` до 1024) |
| `pre_variants_are_bit_identical` | `geometric_partials_pre` / `geometric_peak_pre` == оригінали **бітово** для `powi_pos(r, n±1)` (гарантія кешу fast path) |
| `geometric_matches_naive_sum` | `S_n` vs `Σ rᵏ cos(kx)` для `r ∈ {0.3…0.999}`, `n` до 1024 |
| `dirichlet_peak_and_dc` | пік = `n`, середнє за період `< 10⁻²` |
| `batched_x4_matches_scalar` | `geometric_partials_x4` полейнно ≈ `geometric_partials` у межах `5·10⁻⁶·n + 10⁻⁶` (батч-шлях не гарантує біт-ідентичність зі скаляром — він поза детермінованим гарячим трактом), `n` до 1500, `r` до 1.0 |
| `frac_partial_at_one_equals_the_next_integer_partial` | `geometric_partials_pre_frac(…, frac=1.0)` == `S_{n+1}` (`< 10⁻⁹`) — доводить, що дробовий член це **точно** наступна гармоніка скінченної суми, не апроксимація |
| `hump_matches_the_naive_weighted_sum_and_bumps_the_mids` | `geometric_hump_pre` == `Σ(aᵏ−bᵏ)cos(2πkp)` (пряма сума, `< 10⁻⁶·n`); вага `aᵏ−bᵏ` дійсно піка́є в середніх партіалах (±2 від очікуваного `k`), не на `k=1`; `geometric_hump_peak` = `Σ` ваг, `> 0` (`04 §0.2`) |
| `geometric_reduces_to_fundamental_for_small_r` | `r = 10⁻³` → нормований вихід ≈ `cos(x)` у межах `5·10⁻³` |

### `character` (10)

| Тест | Що доводить |
|---|---|
| `clean_params_are_bit_identity` | `CLEAN` → `process(x) == x` бітово, на 2000 значеннях |
| `set_params_sanitises_hostile_fields` | NaN-поле → `0.0` (значення `CLEAN` для кожного поля), тож all-NaN зводиться до чистого й зберігає побітову тотожність; `±∞` / поза діапазоном → найближча межа. Без цього `is_clean` (`NaN <= 0.0` = false) пропускав би NaN у вейвшейпер |
| `tanh_pade_joins_the_clamp_smoothly` | нахил `tanh_pade` одразу перед клампом `±3` `< 2·10⁻³`, одразу за ним `< 10⁻⁶` (немає зламу першої похідної, який давав кламп `±4`); жодного перельоту `±1` на `x ∈ [0, 50]` |
| `sample_and_hold_state_is_cleared_on_reset` | після навантаженого drive+downsample та `reset()` перший семпл на тишу = `0` бітово (S&H `hold` не тягне хвіст попередньої ноти) |
| `dc_blocker_time_constant_matches_between_1x_and_2x_paths` | загасання DC-зсуву через `process` (1×) і `process_hq_pair` (2×) збігається на матчнутих лічильниках викликів — доводить `√`-масштабування коефіцієнта DC-blocker'а на 2×-шляху (без цього падає на семплі 2000 з розбіжністю `0.263` проти `0.097`) |
| `tiny_downsample_is_bypassed_not_jittered` | `downsample = 9·10⁻⁵` (щойно під bypass-порогом `10⁻⁴`) бітово ідентичний `downsample = 0.0` на 5000 семплах (без порогу падає точно на семплі 741, де мав би спрацювати пропуск S&H-лічильника) |
| `drive_adds_energy_but_stays_bounded` | `drive 0.8` піднімає тихий сигнал (пік `> 0.3`), лишається `|y| ≤ 1.05` |
| `fold_and_grit_stay_finite_and_bounded` | усі 5 стадій разом на 48000 семплів → скінченне, `|y| ≤ 1.2` |
| `hq_path_is_bounded_and_reduces_alias_energy` | 2×+децимація на тоні біля Найквіста в фолдер → менше LF-енергії (аліасів), ніж 1× |
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

### `env` (5)

| Тест | Що доводить |
|---|---|
| `ar_shape_when_sustain_is_full` | attack сягає 1, sustain тримає, release падає до `< 10⁻³` |
| `decays_to_sustain_and_holds` | decay осідає на `sustain = 0.4` у межах `0.02` |
| `zero_sustain_is_percussive_and_frees` | `sustain = 0` → голос стає Idle навіть при затиснутій ноті |
| `hostile_stage_times_still_progress_to_idle` | `stage_time_s` клампить `[0.5 мс, 600 с]`: `+∞` / `1e30` → `600`, NaN / `−∞` / від'ємне → `0.5 мс`. Жоден коефіцієнт стадії не стає `0` → `+∞`-attack/decay/release не лишає голос застряглим і чутним |
| `monotone_attack_then_nonincreasing_release` | attack монотонно росте, release монотонно спадає |

### `lfo` (4)

| Тест | Що доводить |
|---|---|
| `all_shapes_stay_in_range_and_have_zero_mean` | усі форми `∈ [−1,1]`, середнє за цикл `< 0.02` |
| `shapes_are_phase_aligned_at_start` | sine, triangle, saw усі `≈ 0` у фазі 0 |
| `triangle_and_saw_hit_their_peaks` | пік `> 0.95`, мін `< −0.95` |
| `free_run_mode_survives_retrigger` | `FreeRun` — `retrigger()` не чіпає фазу; `Retrigger` (дефолт) — скидає в 0 |

### `voice` (15 + 1 `#[ignore]`)

| Тест | Що доводить |
|---|---|
| `output_stays_bounded_across_the_range` | `f₀ ∈ {20…12000}`: стерео-пік `∈ (0.05, 1.5]`, скінченне |
| `polyblep_saw_and_triangle_are_bounded_and_shaped` | `Saw`/`Triangle` на `f₀ ∈ {55, 220, 3000}`: `\|y\| ≤ 1.6`, енергія над Найквістом `< 2 %·h₁`, гармоніки спадають; трикутник — парні `< 15 %`, `h₃/h₁ ∈ [0.06, 0.22]` (≈ `1/9`) |
| `polyblep_waves_are_flat_into_the_sub_bass` | пилка/трикутник на `f₀ ∈ {27.5, 55, 220}` Гц: фундаментал `±0.5` дБ від ідеального рівня (`2/π` / `8/π²`) — доводить відсутність HPF (плаский відгук до DC) |
| `unrouted_lfo_does_not_affect_output` | голос з LFO на якійсь частоті, але routing `= 0`, рендериться **бітово** так само, як без LFO (fast path не тикає LFO) |
| `lfo_to_cutoff_and_fm_stay_bounded` | усі 4 цілі роутингу разом на filtered+FM голосі → скінченне, `\|y\| ≤ 2.5` (резонансний SVF на швидкому свіпі перевищує unity — це реально) |
| `free_run_lfo_phase_survives_note_on` | `FreeRun` vs `Retrigger` голос після note-on посеред циклу LFO дають **різний** вихід (FreeRun не рестартує вібрато) |
| `geom_and_pan_caches_track_changing_params` | після зсуву `rolloff` + `pan` голос сходиться (`< 1e-4`) до значень свіжого голосу, стартованого прямо на цих параметрах → кеші інвалідуються коректно |
| `equal_power_pan_splits_correctly` | hard-left «протікання» `< 5 %`; центр збалансований `< 5 %` |
| `free_running_phase_survives_note_on` | `free_running=true` → фаза не змінилась на `reset()`; `false` → фаза = 0 |
| `declick_ramps_in_from_near_zero` | перший семпл після `reset()` тихіший за пік перших 64 |
| `pitch_bend_and_lfo_stay_finite` | bend `+2 st` + LFO вібрато `25 ct` → пік `≤ 1.5` на 96000 семплів |
| `partial_limit_caps_but_nyquist_still_wins` | `set_partial_limit` нижче Найквіста → `max_partials()` == ⌊стеля⌋; вище → Найквіст усе одно кепує; `0.0` → кламп до 1; біля Найквіста (n=1) стеля моот |
| `partial_frac_fades_in_the_next_partial` | стеля `n.5` → DFT-магнітуда `(n+1)`-ї гармоніки ≈ пів-значення проти стелі `n+1.0` (неперервний свіп, не сходинка); при стелі `n.0` вона відсутня; коли зв'язує Найквіст — `frac` скидається (не аліасить) |
| `expr_brightness_tilts_the_spectrum_and_zero_is_inert` | понотний зсув `+0.3` / `−0.35` до `rolloff 0.7` → відношення 8-ї гармоніки до фундаменталу росте `> 2×` / падає `< 0.5×`; зсув `0.0` рендериться **бітово** так само, як без експресії |
| `formant_adds_a_movable_mid_spectrum_bump_and_zero_is_inert` | при `rolloff 0.4` формант `0.42` піднімає 6-ту гармоніку `> 8×`; низький формант тримає енергію на партіалі 4, високий зсуває її на 15; `formant 0.0` рендериться **бітово** як без форманту (`04 §0.2`) |
| `phase_accumulators_do_not_drift` `#[ignore]` | `10⁹` семплів vs Kahan-еталон: похибка частоти несучої `< 10⁻³` ppm (виміряно `5·10⁻⁹`), FM так само (§3) |

### `poly` (25)

| Тест | Що доводить |
|---|---|
| `midi_pitch_reference` | `midi_to_hz(69)=440`, `(60)≈261.63`, `(33)≈55` |
| `set_partial_limit_darkens_the_whole_synth` | стеля на гармоніки фанаутиться в усі голоси: DFT-магнітуда 24-ї гармоніки падає `< 5 %` при limit=6 — і для утримуваної ноти (live-fanout), і для ноти, тригернутої після встановлення стелі (`trigger_one`) |
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
| `per_note_brightness_addresses_one_key_and_leaves_the_others_alone` | яскравість, спрямована на клавішу `a`, піднімає нахил (8-ма/фундаментал) **лише** ноти `a` (`> 3×`); спектр ноти `b`, що звучить поряд, не рухається (`< 2 %`) — понотна адресація |
| `channel_brightness_moves_every_sounding_note` | `set_channel_brightness(1.0)` при глибині `0.4` → нахил звучної ноти яснішає `> 3×` (тиск каналу — спільний на всіх) |
| `brightness_depth_zero_leaves_the_synth_bit_identical` | глибина `0.0` + `set_note_brightness` + `set_channel_brightness` → **бітово** той самий вихід, що й без експресії, на 8000 семплах (вимикаюче значення справді no-op) |
| `formant_fans_out_to_every_voice_and_zero_is_bit_identical` | `set_formant` фанаутиться і в утримуваний голос (`trigger_one`), і в свіжу ноту — 6-та гармоніка `> 6×` при `rolloff 0.4`; `formant 0.0` → **бітово** незмінний вихід на 8000 семплах |
| `wildcard_note_brightness_is_ignored_not_a_panic` | `set_note_brightness(255, …)` / `(200, …)` (CLAP wildcard, поза таблицею) → без паніки, вихід скінченний |
| `lowest_sounding_hz_tracks_the_bottom_note` | `PolySynth::lowest_sounding_hz` → `0` коли тихо; A4 → 440, потім A3 → 220 (вища нота не рухає підлогу); слідує тюнінгу (`equal(12, 432, 69)` → A2 = `108`); після `all_notes_off` → `0`. Для спектр-дисплея, не на рендер-шляху |
| `representative_cutoff_tracks_the_filter_envelope` | `PolySynth::representative_cutoff` → `0` коли тихо; LP зі зрізом 500 Гц + `+4` окт filter-envelope, A3: за ~40 мс атаки зріз піднявся `> 1.5×` і `> 1500` Гц, лишається скінченним `< 30 кГц`; після `all_notes_off` + 1 с → `0`. Живить живу криву фільтра в редакторі |
| `representative_rolloff_moves_with_lfo_to_brightness` | `PolySynth::representative_rolloff` → `0` коли тихо; при стійкій яскравості без LFO осідає на базу (`0.6 ± 0.02`); повільний глибокий LFO→brightness розгойдує `r` на `> 0.2` в межах `[ROLLOFF_MIN, ROLLOFF_MAX]`; після `all_notes_off` → `0`. Живить нахил гребінки |
| `render_is_block_size_independent_bit_for_bit` | скриптований прохід (FM + унісон + LFO + фільтр, 6 подій на точних кадрах) хешується **побайтово однаково** через `render_block` розміром 7 / 64 / 256 / 512 / весь блок і посемпловий цикл. «Freeze == realtime»; фіксує гарантію до інтеграції векторного block-x4 (`09`) |
| `zero_latency_hq_off_and_exactly_16_samples_hq_on` | `PolySynth::HQ_LATENCY == 16`, `Voice::HQ_LATENCY == 3` (compile-time); HQ-off побайтово тотожний незалежно від того, чи вмикали HQ (жодної залишкової лінії затримки); крос-кореляція стабільного тону HQ-off vs HQ-on дає лаг **рівно 16** |

### `tests/spectrum.rs` — інтеграційні (6)

| Тест | Що доводить |
|---|---|
| `closed_form_equals_bruteforce` | `D_n` vs пряма сума, `n` до 2048, `< 5·10⁻⁸·n + 10⁻⁶` |
| `geometric_is_a_true_finite_sum` | усічення на `n` vs на `4n` збігаються (`< 10⁻⁶`) → форма скінченна, не нескінченна |
| `rendered_voice_does_not_alias` | DFT рендеру @440 Hz, `r=0.995`: енергія на `f₀` та 10-й гармоніці присутня; `< 10⁻⁴` вище клампу (54 гарм.) та в дзеркальних цілях |
| `partial_limit_truncates_the_spectrum_cleanly` | `set_partial_limit(12)` при f₀=220, r=0.995 (Найквіст дав би ~109): DFT показує енергію до 12-ї гармоніки, `< 10⁻⁴` від 13-ї вгору — реальне обрізання, не фолдинг (кламп після Найквіста) |
| `default_partial_limit_is_bit_identical` | голос без `set_partial_limit` і голос, явно виставлений на `2048.0` (`frac = 0`), рендерять **побайтово** однаковий блок — деф. бере цілочисельну гілку `geometric_partials_pre`, крос-платформний хеш не зачеплено |
| `cost_is_flat_in_partial_count` | час(1200 гарм.) / час(3 гарм.) `< 25×` (не `~400×`) |

### `tests/cross_platform_bit_exact.rs` — інтеграційний (1)

| Тест | Що доводить |
|---|---|
| `rendered_signal_is_bit_identical_across_architectures` | 100 мс рендеру `PolySynth<8>` через весь тракт (унісон 4 + drift + FM + feedback + 4 маршрути LFO + резонансний Low SVF + drive/bias/fold/crush/downsample), зі скриптованими note-on/off та pitch-bend; біти кожного семпла згортаються в FNV-1a хеш і звіряються з константою, знятою на `x86_64-pc-windows-msvc`. Будь-яка розбіжність в 1 ULP на ~9600 семплах змінює хеш. Зелений на x86-64, `aarch64-unknown-linux-gnu`, `armv7-unknown-linux-gnueabihf` (§6) |

### `tests/stress.rs` — інтеграційні, RT-safety (11)

Ворожий доказовий набір: контракт аудіо-шляху («ніколи не паніка / не
алокація / не блокування / не нескінченний / не необмежений семпл — *хай що*
подасть хост») перевіряється атаками, а не довірою.

| Тест | Що доводить |
|---|---|
| `nan_and_inf_into_every_polysynth_setter_stays_finite` | для кожного з `{NaN, ±∞, субнормаль, ±1e300, −0}`: усі публічні сеттери `PolySynth` по черзі отримують це значення, потім 8000 семплів — скінченні, `\|y\| ≤ 4`; далі рушій ще здатен озвучити свіжу ноту й дійти до тиші (жоден голос не застряг) |
| `nan_and_inf_into_every_voice_setter_stays_finite` | те саме для всіх ~18 публічних сеттерів standalone `Voice` |
| `parameter_storm_at_sample_rate` | ~300 k семплів, ~10 параметрів змінюються **щосемпла** випадково-в-діапазоні (детермінований LCG), акорд тримається, HQ тумблиться — скінченне, обмежене |
| `note_event_storm_never_exceeds_the_pool_and_recovers_to_silence` | 20 k перемішаних note-on/off/choke на 16-голосний пул з унісоном 4 → `active_voice_count() ≤ 16` завжди; після `all_notes_off` + 4 с → 0 голосів, хвіст `< 10⁻⁴` |
| `hostile_envelope_times_cannot_strand_a_voice` | `+∞` / `1e30` / NaN / від'ємні attack+decay+release → голос усе одно звільняється (жоден коеф. стадії не `0`) |
| `sample_rate_extremes_with_the_whole_tract_lit` | увесь тракт (осц + Partials + expr + FM + feedback + character + Band SVF + фільтр-env + LFO×4 + унісон 6 + HQ) на `8000` та `768000` Гц → скінченне, обмежене |
| `hq_toggled_every_few_samples_under_a_hot_signal` | `set_hq` тумблиться кожні 7 семплів під перевантаженим drive+fold+FM сигналом, 120 k семплів → скінченне (дециматор скидається щоразу) |
| `reset_while_sounding_is_immediately_clean` | `reset()` посеред акорду → 0 голосів одразу, вихід `< 10⁻⁶` |
| `long_run_holds_finite_bounded_and_does_not_drift_in_level` | ~2.1 M семплів (~44 с) утримуваного акорду з повільним LFO: RMS раннього vs пізнього вікна `∈ [0.5×, 2×]` — рівень не пливе ні в нуль, ні вгору |
| `subnormal_and_zero_frequency_are_handled` | `set_frequency` на `{0, −0, MIN_POSITIVE, 1e-20, −50}` → скінченне |
| `waveform_switching_mid_note_stays_bounded` | Geometric↔Saw↔Triangle перемикається кожні 11 семплів на звучній ноті → скінченне |

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
| Тести | `cargo test` | 108 / 108 (90 юніт + 18 інтеграційних) |
| no_std бінарник | `cargo build --no-default-features --release` | `harmonic_core.dll` (~14 КБ) + `.lib` |
| Плагін | `cargo xtask bundle harmonic_synth --release` | `.vst3` + `.clap`; `clap_entry` присутній, VST3 має `GetPluginFactory`/`InitDll`/`ExitDll` |

---

## 5. RT-safety

### 5.1. `grep` — статично

```
$ grep -nE 'unwrap\(\)|expect\(|panic!' src/*.rs | grep -v '#\[cfg(test)\]' ...
```
→ збіги **лише** у `#[cfg(test)]`-модулях (`env.rs` тести). Нуль у гарячому
шляху. `[profile.release] panic = "abort"` в **обох** крейтах.

### 5.2. `tests/stress.rs` — ворожий доказовий набір (11 тестів)

Контракт аудіо-шляху — **ніколи** не паніка / не алокація / не блокування /
не нескінченний / не необмежений семпл, *хай що* подасть хост — перевіряється
атаками, не довірою. `NaN` / `±∞` / `±1e300` / субнормаль по черзі в **кожен**
публічний сеттер `PolySynth` і `Voice`; шторми параметрів і нот на швидкості
семплу; крайні частоти дискретизації; HQ під навантаженням; прогін на 2 M
семплів. Повний перелік — §2. Кожен сценарій вимагає скінченного обмеженого
виходу **і** що ворожий вхід не лишає голос застряглим.

Набір знайшов і зафіксував (корінь, не симптом):

| Місце | Було | Стало |
|---|---|---|
| `Adsr::set` | `+∞` / величезний час стадії → коеф. `0` → голос навіки в Attack/Decay/Release | `stage_time_s` клампить `[0.5 мс, 600 с]` |
| `Character::set_params` | NaN-поле проходило повз `is_clean` (`NaN <= 0` = false) → NaN у вейвшейпері назавжди | санітизація: NaN → `0.0` (= `CLEAN`), `±∞`/поза діапазоном → межа |
| `Voice::set_start_phase` / `Lfo::set_phase` / `set_unison_drift_phase` | `1e300 − floor(1e300)` (floor точний лише до `2⁶³`) → фаза поза `[0,1)` → осц. видає non-finite | `trig::wrap01`: non-finite / нередуковне → `0.0` |
| `PolySynth::set_gain` | `+∞` → `mix · ∞ = NaN` на нульовому семплі мікса; `1e300 as f32 = ∞` — те саме | кламп `[0, 64]`, тільки скінченне |
| `Voice::set_partial_limit`, `PolySynth::set_partial_limit` | `f32::clamp(NaN,…)` повертає NaN → `partial_frac` = NaN | NaN → деф. `2048` |
| `PolySynth::set_unison`, `Lfo::set_rate` | `f64::clamp(NaN,…)` = NaN; `NaN < 0.0` = false | явна NaN-гілка (`nan_clamp` / `is_nan`) |

Клампи скидання NaN уже стояли на `Voice::set_frequency/gain/pan/…`,
`Svf::set_cutoff/resonance`, `env::clamp01`, `PolySynth::set_gain`(NaN-частина)
— набір закрив решту периметра публічного API. Усі виправлення лишають
чистий / дефолтний шлях **побайтово** незмінним (крос-платформний хеш §6-bis
не зачеплено).

### `harmonic_license` — ліцензійний keyfile (9, `cargo test -p harmonic_license [--features sign]`)

| Тест | Що доводить |
|---|---|
| `the_embedded_pubkey_is_a_valid_ed25519_point` | `LICENSE_PUBKEY` парситься як коректна точка Ed25519 — форсує заміну dev-ключа на робочий перед релізом |
| `the_committed_sample_key_verifies` | `SAMPLE_LICENSE.key` (у репо) верифікується проти `LICENSE_PUBKEY`, `tier == "studio"`, вотермарк непорожній |
| `a_signed_keyfile_verifies_and_carries_the_watermark` | згенерована пара → підпис → верифікація повертає точно ті самі поля; `watermark()` = `"Ім'я <email>"` |
| `editing_any_signed_field_breaks_it` | зміна будь-якого підписаного поля (name / email / order / tier / issued) → `SignatureMismatch` |
| `a_key_from_a_different_seller_is_rejected` | верифікація проти чужого публічного ключа → `SignatureMismatch` |
| `malformed_files_are_rejected_not_panicked` | порожній / без `product` / без email / чужий product / нехекс-підпис / порожнє ім'я → `Err`, без паніки |
| `hex_round_trips` / `field_needs_a_real_separator` / `watermark_without_email` | хелпери: hex-кодек, парсер полів (`name` не матчить `name_of_thing`), вотермарк без email = лише ім'я |

### `harmonic_synth` — плагінні (31, `cargo test -p harmonic_synth`)

| Тест | Що доводить |
|---|---|
| `analyzer::meter_is_calibrated_to_dbfs` | повношкальний тон на `0.44·f_s` (центр смуги) → метр читає `≈ 0` dBFS (`−3…+2`) — пін для `NYQ_GAIN_COMP` (`04 §1.8`) |
| `analyzer::meter_ignores_a_clean_low_tone_and_catches_near_nyquist_energy` | чистий тон 1 кГц → `< −55` dBFS; тон `−12` dBFS у смузі фолду → `−12±4`; розділення `> 35` дБ |
| `analyzer::meter_decays_after_the_energy_stops` | після припинення енергії метр падає `> 30` дБ (envelope-фоловер відпускає) |
| `editor::morph_endpoints_are_exact_and_midpoint_blends` | A/B морф: `pos = 0` → **рівно** A, `pos = 1` → **рівно** B (без дрейфу); середина = півсуми; `pos` клампиться (не екстраполює); відсутній у слоті параметр → `None` (не чіпається). `07 §18` |
| `editor::partial_comb_weights_match_the_engine_shape` | вага партіала спектр-гребінки `rᵏ + h·(aᵏ−bᵏ)` (те саме, що `voice.rs::geom_osc`): без горба — точно `rᵏ`, монотонно спадає; горб форманти піднімає партіал `≈ kc` над чистим `rᵏ` і знову спадає вище центру |
| `editor::hover_state_survives_a_stale_leave` | `apply_hover`: наведення на рядок Brightness → Partials → «застаріле» покидання Brightness (прийшло після входу в Partials) **не** скидає стан; справжнє покидання Partials скидає. Кодування `-1 - which` для leave |
| `editor::hover_encoding_covers_the_filter_rows` | те саме кодування `-1 - which` для рядків Cutoff (3) та Resonance (4): enter Cutoff → Resonance, застаріле покидання Cutoff ігнорується, справжнє покидання Resonance скидає |
| `editor::filter_response_curve_matches_the_svf_shape` | аналітична АЧХ `Spectrum::filter_response` збігається за формою з рушійним `Svf` (`filter.rs`): `Off` — рівно `1.0` скрізь; LP — плоска смуга пропускання та `≈ −12` дБ/окт (×3.5…5.5 на октаву); резонанс піднімає зріз `> 8×`; HP дзеркалить; BP пікує на зрізі, Notch занулює; скінченна та `≥ 0` на ворожих входах |
| `editor::unison_smear_is_constant_width_on_the_log_axis` | `Spectrum::unison_half_width_px`: нуль detune (або нульовий діапазон) → 0 px; лінійна за detune (×2 → ×2 px); октава detune = рівно одна октава осі — тобто розмазування партіала стале в пікселях незалежно від `k` |
| `rando::code_round_trips_and_normalises_look_alikes` | `decode(encode(seed)) == seed` для крайніх seed; case-insensitive; Crockford `I/L→1`, `O→0`; відкидає невірну довжину / символ. `07 §19` |
| `rando::value_for_is_deterministic_in_range_and_varies` | той самий seed → той самий патч (побайтово вектор); різні seed → різний; кожен параметр у своєму вікні `SPEC`; нерандомізований (`hqmode`) → `None` |
| `rando::distribution_spans_each_window` | по 400 seed кожне широке вікно покривається зверху донизу (`< lo + 0.15·span` та `> hi − 0.15·span`) — груба перевірка якості хешу |
| `presets::every_preset_names_only_real_parameters` | кожен `#[id]` у `PRESETS` — реальний параметр (типо в id → лоадер тихо пропускає), значення скінченні; банк `≥ 20` пресетів; `[0]` = «Init» без оверрайдів |
| `presets::every_preset_renders_bounded_non_silent_audio` | кожен пресет застосований у `PolySynth<8>` (мапінг plain→рушій дзеркалить `process()`), акорд 1 с: скінченне, пік `≤ 1.01`, RMS `> 2·10⁻³`; після `all_notes_off` + 6 с хвіст `< 5·10⁻³` (реліз працює) |
| `presets::bass_presets_are_actually_bassy` | «Deep Sub» / «FM Bass» на ~55 Гц: енергія `40…300 Гц` `> 3×` енергії `2…6 кГц` — назви не брешуть |
| `tuning::equal_at_440_is_the_engine_default` | `build(Equal, root, 440)` → `is_equal_440()` при будь-якому root; `ref = 432` → вже ні (справжній ретюн) |
| `tuning::just_intonation_rooted_at_c_keeps_c_at_12tet_and_purifies_the_fifth` | JI від C: C4 не зрушений від 12-TET, G4 — чиста `3:2`, E4 — чиста `5:4` |
| `tuning::root_moves_which_key_is_pure` | JI від A: A4 = 440, E5 (квінта вгору) стає `3:2` — вибір тоніки переносить, яка клавіша чиста |
| `tuning::edo_and_bohlen_pierce_have_the_right_period` | 19-EDO і 31-EDO: `edo` клавіш = октава; Bohlen-Pierce: 13 клавіш = `×3` |
| `tuning::every_scale_gives_finite_positive_frequencies_across_the_keyboard` | усі 8 шкал × 12 тонік × 128 нот → скінченна додатна частота |
| `tuning::parses_a_ratio_scl_and_matches_the_built_in_pythagorean` | реальний `.scl` із відношеннями (`3/2`, `9/8`, `2/1`) → 12 ступенів, квінта = `701.955` ц, період `1200` ц; round-trip через `to_compact` / `expand_compact` |
| `tuning::parses_a_cents_scl` | `.scl` із центовими значеннями (`350.0`, `1200.000`) парситься як центи напряму |
| `tuning::build_scala_puts_the_fifth_on_a_pure_3_2` | `build_scala(compact, root=C, 440)` → C4 як 12-TET, G4 — чиста `3:2`, октава подвоюється |
| `tuning::malformed_scl_is_rejected_not_panicked` | порожній / без лічильника / невірна кількість / `0/0` / від'ємний період → `Err`, без паніки; `build_scala("")` / `("garbage")` → `None` (фолбек на enum) |
| `tuning::a_scale_bigger_than_the_cap_is_truncated_not_rejected` | 100-нотна шкала → обрізана до `MAX_SCALA_DEGREES = 64`, період збережено, усі 128 нот скінченні |
| `tuning::parses_a_kbm_and_reports_dead_keys` | реальний `.kbm` (12-клавішний патерн «білі клавіші», чорні = `x`) → `entries == [0,-1,1,-1,2,3,-1,4,-1,5,-1,6]`, `mid_note`/`ref_note`/`ref_hz` з файлу; round-trip через `to_compact_kbm` / `expand_kbm` |
| `tuning::build_with_kbm_kills_the_black_keys_and_keeps_the_diatonic_scale` | мапа «білі клавіші» над 7-нотним JI-мажором: A4 (реф `.kbm`) = точно 440, E4 (ступінь 2) — чиста `5:4`, октава подвоюється, усі 128 нот (і мертві) скінченні, `is_equal_440()` = `false` |
| `tuning::build_with_kbm_falls_back_to_a_chromatic_scale_when_no_scl` | `.kbm` без `.scl` → мапить на 12-EDO хроматику; 6-й запис патерну → хроматичний ступінь 3 (300 ц), реф = 440, мертва клавіша скінченна |
| `tuning::malformed_kbm_is_rejected_not_panicked` | порожній / обрізаний / розмір 0 / реф-частота 0 / усі клавіші мертві / нечисловий запис → `Err`, без паніки; `build_with_kbm("", "")` / `("", "garbage")` → `None` |
| `tuning::kbm_formal_octave_degree_sets_the_repeat_interval` | «формальна октава» = ступінь 2 (чиста квінта): один повний повтор мапи вгору = `3:2`, не `2:1` |
| `load_license_honours_the_explicit_path_and_verifies_it` | `HARMONIC_SYNTH_LICENSE` → `SAMPLE_LICENSE.key` → `load_license()` повертає верифіковану ліцензію (`tier == "studio"`, вотермарк `name <email>`); шлях-оверрайд працює, читання поза аудіо-потоком |

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

`Editor` / `Open editor whilst processing` перевіряють лише, що редактор
**відкривається й малюється без крашу** — не *як* він виглядає. Дефект
компонування (усі 7 груп параметрів одна поверх одної) проліз повз обидва
валідатори й був знайдений щойно в живому REAPER; фікс і корінь —
`11_DAW_CHECKLIST` (журнал, сесія 2026-09-06) та `03 §GUI`.

---

## 6-bis. Платформна бітова ідентичність — виконано

`scripts/cross-verify.{sh,ps1}` (Docker + QEMU, `rust:1.97-slim`) проганяє
**весь набір `harmonic_core`** на емульованих ARM-таргетах:

| Таргет | `f64`-FPU | Результат |
|---|---|---|
| `aarch64-unknown-linux-gnu` | AdvSIMD/FP | **108 / 108 pass** (90 юніт + 18 інтеграційних) |
| `armv7-unknown-linux-gnueabihf` | VFPv3-d16 — **тотожний Cortex-M4F** | **97 / 97 pass** |
| `wasm32-unknown-unknown` (Node) | нативний wasm `f64` | **хеш = референс** (`scripts/verify-wasm.mjs`) |

Сценарій рендеру та константи-хеші живуть у `harmonic_core::verify`
(`render_verification_at(sr, …)` / `verify_hash` / `VERIFY_HASH` /
`VERIFY_HASH_96K`), тож інтеграційний тест, ARM-звірка й wasm-звірка
проганяють **той самий байт-у-байт прохід**.
`rendered_signal_is_bit_identical_across_architectures` звіряє хеш рендеру
всього тракту з референсом, знятим на `x86_64-pc-windows-msvc`, **на двох
частотах — 48 і 96 кГц** (щоб «на будь-якій частоті» мало підтвердження):
**дельта `= 0.0`**. wasm-звірка (`hc_verify_*` експорти + `verify-wasm.mjs`)
перевіряє і власний хеш модуля, і незалежний JS-фолд байтів — обидва
`= 0xc7f786d40586da75` (48 кГц). Дрейф фазового акумулятора
(`voice::phase_accumulators_do_not_drift`, `DRIFT_SAMPLES=2·10⁷`) теж
збігається до останньої значущої цифри (`4.566·10⁻¹⁰` обертів carrier,
`3.483·10⁻¹⁰` FM) на x86-64 та `aarch64`.

Обґрунтування: жоден гарячий шлях не використовує `libm`, `mul_add` чи FMA-
контракцію; x86-64 на SSE2 (без x87 80-біт); касти `(x as i64)` насичувані
на рівні мови. IEEE-754 `+ − × ÷` коректно округлені однаково на SSE2,
VFP/NEON та wasm → результат мусить збігатися, і тепер це **виміряно**.

Compile-only (у `cross-verify.sh`): `thumbv7em-none-eabihf` (Cortex-M4F/M7 —
Daisy Seed), `thumbv6m-none-eabi`, `riscv32imac-unknown-none-elf`,
`aarch64-unknown-none` — усі збираються чисто `--no-default-features
--release`. Ще ні: реальне залізо Cortex-M.

Апаратура: Docker Desktop 29.7, QEMU user-mode через `binfmt_misc`.

---

## 7. Що НЕ покрито

- **Живий DAW** (Ableton, Bitwig, Reaper, Logic) — не запускалось (лише
  pluginval / clap-validator, §6).
- **Регресійний тест на CLAP `ext_state_load`-фікс** — сам фікс перевіряється
  лише `clap-validator` (у `cargo xtask validate`, не в `cargo test`).
- **ARM під QEMU + wasm32 під Node — покрито** (§6-bis: `aarch64` + `armv7-hf`
  108/108, `wasm32` хеш = референс). **Не покрито:** реальне залізо Cortex-M
  (`thumbv7em` / `thumbv6m`), прогін під RISC-V — усе крос-компілюється чисто
  (compile-check у `cross-verify.sh`), але на залізі не проганялось.
- **Частоти дискретизації поза `[8000, 768000]` Hz** — тепер клампляться зі
  статус-кодом (не тихо), але сам кламп-шлях у реальному хості не тестований.
- **RT-safety — покрито** (§5.2: ворожий вхід у весь публічний API, шторми,
  крайні режими, 2 M семплів). **Не покрито:** справжній property-based
  fuzzer (`cargo-fuzz` / `proptest`) — `tests/stress.rs` детермінований і
  скриптований, не рандомізований пошук; вимірювання денормалей за часом
  (тестується коректність тиші, не її вартість у циклах).
- **Автоматизація параметрів на межі блоку у справжньому `nih-plug`-хості** —
  marshalling самого фреймворку не тестований юнітами (покрито `pluginval
  --strictness 8`, який міняє розмір блоку й SR). На рівні рушія
  блок-незалежність **побайтова** — `poly::render_is_block_size_independent`.
- **`geometric_partials_x4_simd`** на nightly перевірено лише що
  **компілюється** — числова еквівалентність скаляру не має окремого тесту
  (батч-версія `geometric_partials_x4` — має).

## 8. CI

`.github/workflows/ci.yml` (push / PR → `main`): `harmonic_core` test +
обидва clippy-конфіги + `no_std` build + wasm32 build & `verify-wasm.mjs` +
compile-check `thumbv7em`/`thumbv6m`/`riscv32imac`/`aarch64-none`; nightly
`portable-simd` clippy+build; `harmonic_synth` test + clippy + `xtask bundle`
(артефакт Linux); `cross-verify.sh` (ARM QEMU, окрема джоба). **Не в CI:**
`cargo xtask validate` (pluginval / clap-validator тягнуть зовнішні бінарники —
локально), `cargo fmt --check` (стиль передує rustfmt 1.9). `09`.
