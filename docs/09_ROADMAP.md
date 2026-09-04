# 09 — Дорожня карта розвитку

**Поточний стан:** core stable, 70/70 тестів green (+1 `#[ignore]` дрейф),
проходять біт-у-біт на `aarch64` + `armv7-hf` під QEMU. clippy чистий
(std / no_std / nightly-simd). VST3 — `pluginval` strictness 8 повний прохід
(з GUI-тестами). CLAP — `clap-validator` **35/35** (баг `ext_state_load`
виправлено через `[patch]` на `vendor/nih-plug`, §Б2). Плагін має GUI
(`nih_plug_vizia`). Роадмапа закрита; лишилось В2 (ручна DAW-валідація) та
публічний upstream-PR по Б2.

**Прогрес:** ✅ Б3 (крос-компіляція + bit-exact на ARM, RFC-15) · ✅ А1 (PolyBLEP пила/трикутник) ·
✅ Б2 (фікс CLAP-обгортки nih-plug: `[patch]` на vendor → `clap-validator`
35/35; публічний PR — за користувачем) ·
✅ Б1 (clean-voice fast path: +25 % на голос, +80 % поліфонії) ·
✅ А2 (LFO retrigger/free-run + матриця: LFO→cutoff, LFO→FM index) ·
✅ В3 (числовий дрейф виміряно: `5·10⁻⁹` ppm за 5.8 год) ·
✅ А3 (дрейф-LFO унісону: образ «дихає» ±11 % ширини) ·
✅ В1 (GUI `nih_plug_vizia`: усі параметри + спектр-дисплей).

Три вектори. У кожному пункті: **Проблема / Рішення / Файли / Оцінка /
Критерій готовності (DoD)**.

---

## Вектор А — Розширення DSP та звукового потенціалу

Осцилятор зараз дає лише band-limited імпульсний потік (`D_n`) та його
геометрично-спадні версії (`S_n`). Для конкуренції з флагманами потрібні
класичні хвилі й глибша модуляція.

### А1. Класичні хвилі без аліасингу — PolyBLEP / PolyBLAMP ✅ ВИКОНАНО

- **Проблема.** `Σ sin(kx)/k` (пила) не має елементарної закритої форми
  (функція Клаузена) — прямий геометричний ряд пилу/трикутник не дає.
- **Історія.** Спершу — leaky-integrated BLIT (Stilson & Smith): інтегрування
  `dirichlet_blit` із витоком. Працювало, але подвійне рекурсивне
  інтегрування трикутника вимагало сильного витоку 2-ї стадії проти втечі →
  **спад суб-басу `−5` дБ на 28 Гц** (недопустимо для комерційного басу).
- **Рішення (фінал).** **PolyBLEP / PolyBLAMP** (Välimäki & Huovilainen 2007):
  наївна хвиля + поліноміальна корекція `±dt` навколо розриву/зламу.
  **Без стану, без рекурсії, без тригонометрії.** `voice.rs::polyblep_saw` /
  `polyblamp_triangle` (вільні `fn`), `poly_blep` / `poly_blamp` хелпери.
  Прибрано 8 полів стану → `Voice` `576 → 512` б.
- **Проведення.** `Waveform` enum, `poly.rs` fan-out, `ffi.rs`, C-header,
  плагін (`OscKind` EnumParam «Oscillator»). Docs 01 §7 / 03 / 04 §11 / 05 /
  06 §3 / 07 §6 / 08.
- **Результат (виміряно, `f₀ ∈ [27.5, 6000]` Гц).**
  - Пила: спектр `1/k` (`h₂ 0.50`, `h₃ 0.33`), `±1`, фундаментал `±0.02` дБ
    від ідеалу до 27.5 Гц, alias `< −90` дБ. **90 M семпл/с.**
  - Трикутник: непарні `1/k²` (`h₃ 0.110`, `h₅ 0.040`), парні `< 0.1 %`,
    `±1`, фундаментал `±0.00` дБ до 27.5 Гц. **77 M семпл/с** (проти ~26 M
    геометричного — тобто **дешевше за несучу**).
- **Компроміс (07 §6).** Не точно band-limited (поліном апроксимує ідеальне
  BLEP-ядро): на `f₀ > 3` кГц фундаментал і верхні гармоніки трохи спадають
  (`−0.11` дБ на 3 кГц, `−0.45` дБ на 6 кГц). Нечутно в музиці; на A7 пилкою
  ніхто не грає. Ігнорують `rolloff` та HQ.
- **Тести.** `voice::{polyblep_saw_and_triangle_are_bounded_and_shaped,
  polyblep_waves_are_flat_into_the_sub_bass}`.

### А2. Поголосна LFO — retrigger + матриця модуляції ✅ ВИКОНАНО

- **Проблема.** LFO не мав режиму key-sync і роутився лише в `rolloff` та
  `pitch`.
- **Зроблено.**
  1. `LfoMode { Retrigger, FreeRun }` (`lfo.rs`) — `retrigger()` не чіпає фазу
     у `FreeRun`; `Voice::set_lfo_mode`, fanout у `poly.rs`.
  2. Матриця розширена на 4 цілі: `set_lfo_targets(to_rolloff, to_pitch_cents,
     to_cutoff_oct, to_fm)`. `lfo_to_cutoff` → `filter.set_cutoff(base ·
     2^(d·m))` (композиться мультиплікативно з фільтровою обгинаючою);
     `lfo_to_fm` → `max(fm_index + d·m, 0)`. Кожна ціль `0` не застосовується;
     усі 4 нулі → LFO не тикається (clean fast path зберігається, бітово).
  3. Плагін: `LFO Sync` (enum), `LFO → Cutoff` (±4 окт), `LFO → FM` (±4).
     C-ABI `harmonic_voice_set_lfo` розширено (+`mode`, +2 цілі).
- **Файли.** `lfo.rs`, `voice.rs`, `poly.rs`, `ffi.rs`, `include/harmonic_core.h`,
  `harmonic_synth/src/lib.rs`. Docs 03/04 §5/05/06/08.
- **Тести.** `lfo::free_run_mode_survives_retrigger`,
  `voice::{lfo_to_cutoff_and_fm_stay_bounded, free_run_lfo_phase_survives_note_on}`,
  `poly::lfo_modulation_stays_bounded` (розширено на всі 4 цілі). 53→56.
- **Voice.** 528 → 552 б (+3 `f64`).

### А3. Стерео-унісон — динамічна декореляція ✅ ВИКОНАНО (drift-LFO; мікро-затримка — свідомо ні)

- **Проблема.** Унісон робить статичне розстроювання + панораму; стерео-образ
  «правильний», але **нерухомий**.
- **Зроблено.** Повільний вільний **дрейф фази на кожен унісон-голос**
  (`voice.rs`: `drift_phase/inc/depth`, `sin_turns_fast`, не ретригериться).
  `Voice::set_unison_drift(rate_hz, depth_turns)` + `set_unison_drift_phase`;
  `PolySynth::set_unison(count, detune, spread, **drift**)` дає кожному голосу
  ставку `~0.12·(0.55 + 0.9·i/n)` Гц (не синхронну), глибину `drift·0.05`
  обертів, стартову фазу `i·φ`. Плагін: `Uni Drift` (%). C-ABI не зачеплено
  (дрейф — на рівні `PolySynth`).
- **Результат (виміряно).** Детюн `0` → віконна ширина нерухома (span `0.00`);
  `drift 0.3/0.7/1.0` → span `0.06/0.15/0.22` (`±11 %` образу), не колапсує.
  `06 §3`, тест `poly::unison_drift_makes_the_image_breathe`.
- **Мікро-затримка на голос — свідомо НЕ додано.** Делей-буфер 0–20 мс/голос
  = `~4 КБ`/голос (`~96 КБ` на `PolySynth<24>`) — ×7 розриває RAM-бюджет
  `08 §2.3` заради ефекту, який дрейф фази вже дає без буфера. Якщо колись
  знадобиться повніший chorus/ensemble — окремим кроком за feature-gate.
- **Voice.** 552 → 576 б (+3 `f64`). Тести 56→57.

---

## Вектор Б — Системна оптимізація та портування

### Б1. «Clean Voice Fast Path» ✅ ВИКОНАНО (per-sample; block-x4 — ні)

- **Проблема.** `render_sample` тикав LFO навіть нероутований і щосемпла
  рахував `powi_pos(r, n±1)` (у `geometric_partials` + `geometric_peak`) та
  `sin_cos` панорами, хоча `r`, `n` і `pan_z` на сталій ноті не рухаються.
- **Зроблено (`voice.rs`).**
  1. **LFO fast path** — коли `lfo_to_rolloff == 0 && lfo_to_pitch == 0`,
     LFO не тикається, `f_eff = freq_z·bend_z`, `roll_eff = rolloff_z`
     (бітово ідентично: множення на `1.0`, кламп-no-op).
  2. **Geom-кеш** — `geom_norm(r, n)`: `r^{n+1}` та пік перераховуються лише
     на зміні `(r, n)`; нові `kernel::geometric_partials_pre` /
     `geometric_peak_pre` приймають готове `powi_pos` (бітово ідентичні
     оригіналам — тест `pre_variants_are_bit_identical`).
  3. **Пан-кеш** — equal-power гейни кешуються поки `pan_z` не рухається.
  6 нових полів `Voice` (+64 б → 528). Тести `unrouted_lfo_does_not_affect_output`,
  `geom_and_pan_caches_track_changing_params`.
- **Результат.** Один голос: +23–26 % (`bench_hc`), тепер плоско по `n`.
  Повний акорд 64 голоси: **~+80 %** (~330 → ~590 голосів realtime-запасу,
  `bench_poly`) — низькі ноти з великим `n` найбільше вигравали від кешу
  `powi_pos`. `06 §3` оновлено.
- **Не зроблено.** Block-x4 (`render_block` через `geometric_partials_x4`) —
  плагін рендерить посемплово (sample-accurate MIDI), тож блок-шлях не на
  гарячому шляху; лишається як опція для офлайн/`render_block`-споживачів.

### Б2. Фікс CLAP-обгортки nih-plug ✅ ВИКОНАНО (локальний `[patch]`; публічний PR — за користувачем)

- **Проблема.** `src/wrapper/clap/wrapper.rs` (rev `de421011`, і на `master`):
  (1) `ext_state_load` не постить `Task::RescanParamValues` → хост не отримує
  `clap_host_params::rescan(CLAP_PARAM_RESCAN_VALUES)` після `load`
  (`state-reproducibility-{basic,binary,buffered}` FAIL);
  (2) `Vec::with_capacity(length as usize)` з невалідованим `length` зі
  стріму → **alloc-abort (`0xc0000409`) — жорсткий crash DAW** на пошкодженому
  пресеті (`state-invalid-random`). VST3-шлях стану проходить `pluginval` —
  тобто це саме CLAP-обгортка.
- **Зроблено.** Патчена копія pinned-дерева у `harmonic_synth/vendor/nih-plug/`
  (тільки `nih_plug` + `_derive` + `_vizia` + `_xtask`), підключена через
  `[patch."…/nih-plug.git"]` у `harmonic_synth/Cargo.toml` (покриває й
  workspace-member `xtask`). Фікс: (1) `wrapper.schedule_gui(Task::RescanParamValues)`
  після успішного `set_state_inner`, як у `set_state_object_from_gui`;
  (2) `try_reserve_exact` замість `Vec::with_capacity`. `--exclude` зі
  `scripts/validate.{ps1,sh}` **прибрано** (заодно `--skip-gui-tests`).
- **Результат.** `cargo xtask validate`: `clap-validator` **35 / 35, 0 failed,
  0 warnings** (було 31 / 4 / 9); `pluginval --strictness 8` (VST3, з
  GUI-тестами) SUCCESS. Корінь, репро, склад vendor — `10_NIH_PLUG_CLAP_BUGS.md`.
- **Не зроблено.** Публічний форк `robbert-vdh/nih-plug` + PR — за
  користувачем. `contrib/nih-plug-clap-state-load-fix.patch` — готовий,
  застосовується на `de421011` і `master`.
- **DoD.** ✅ `clap-validator` 35/35 без винятків, без збоїв, без ворнінгів.
  Після мержу upstream — видалити `vendor/nih-plug` + `[patch]`, бампнути `rev`.

### Б3. Фізична верифікація на залізі ✅ КРОК 1+2 ВИКОНАНО (RFC-15)

- **Проблема.** Бітова ідентичність `no_std`-коду поза x86_64 — була
  очікувана гіпотеза (`08 §3`).
- **Крок 1 ✅.** Крос-**компіляція** чиста під `thumbv7em-none-eabihf`,
  `thumbv6m-none-eabi`, `aarch64-unknown-none`, `riscv32imac-unknown-none-elf`.
- **Крок 2 ✅ (RFC-15).** `scripts/cross-verify.{sh,ps1}` (Docker + QEMU)
  проганяє **весь набір 70/70** на `aarch64-unknown-linux-gnu` та
  `armv7-unknown-linux-gnueabihf` (VFP `f64` тотожний Cortex-M4F). Новий
  інтеграційний тест `cross_platform_bit_exact` звіряє хеш 100-мс рендеру
  всього тракту з x86-64 референсом — **дельта `= 0.0`** на всіх трьох
  архітектурах; дрейф фази теж збігається до останньої цифри.
- **Не зроблено.** Реальне залізо Cortex-M, soft-float `thumbv6m` (M0 без
  FPU), прогін під RISC-V. CI-матриця (`.github/workflows`) — коли буде CI.
- **DoD.** ✅ 3 таргети крос-компілюються; ✅ bit-exact синтез на 2 не-x86
  (aarch64 + armv7-hf) → застереження в `08 §3` / `07 §12` знято.

---

## Вектор В — Комерційний інтерфейс, жива валідація, продуктивність

### В1. GUI на `nih_plug_vizia` ✅ ВИКОНАНО (мінімальний; власний layout — полиш)

- **Проблема.** Плагін — сухий табличний список параметрів хоста.
- **Зроблено.** `src/editor.rs` (`nih_plug_vizia`): заголовок + власний
  `Spectrum` View (30 барів, log-freq x, dB y, читає атоміки щокадру) +
  `GenericUi` з усіма 33 параметрами у `ScrollView`, під невеликий CSS.
  Розмір вікна персиститься (`#[persist] editor_state: Arc<ViziaState>`).
  `src/analyzer.rs` — спектр **без FFT і без нової DSP-залежності**: банк із
  30 резонансних band-pass `harmonic_core::Svf` (Q≈5) + envelope-фоловери,
  результат у `[AtomicF32; 30]` (audio→GUI, lock-free); працює лише коли
  `editor_state.is_open()`.
- **Залежності.** `+nih_plug_vizia` (той самий pinned rev nih-plug; тягне
  baseview / vizia / femtovg / шрифти — перша збірка кілька хвилин) +
  `atomic_float`. `harmonic_core` лишається zero-dependency.
- **DoD.** `pluginval --strictness 8` **без `--skip-gui-tests`** проходить —
  `Editor`, `Open editor whilst processing`, `Editor Automation` виконуються
  й зелені. Усі параметри керуються. `06 §6` оновлено.
- **Полиш (не зроблено).** Власний згрупований layout замість `GenericUi`,
  крива фільтра як окремий графік, ручки (knobs) замість слайдерів — коли
  дійде до візуального брендингу.

### В2. Стрес-тест у живих DAW

- **Проблема.** `pluginval` ✔, але плагін не запускався в реальному
  Ableton / Reaper / Logic / Bitwig / Cubase.
- **Рішення.** Ручний чек-лист по кожному хосту: state recall у межах проєкту,
  синхронізація темпу (коли додамо tempo-sync LFO), зміна буфера на льоту під
  час відтворення, автоматизація, кілька інстансів, sample-rate проєкту.
- **Файли.** `docs/11_DAW_CHECKLIST.md` (протокол готовий) + журнал результатів.
- **Оцінка.** M (ручна робота).
- **DoD.** Чистий журнал по ≥3 хостах; зафіксовані баги — в issue-трекер.

### В3. Довготривалий числовий дрейф ✅ ВИКОНАНО

- **Проблема.** Фазова математика в обертах, wrap щоперіоду — накопичення
  похибки обмежене теоретично (`07`), але не виміряне на годинах.
- **Зроблено.** `voice::phase_accumulators_do_not_drift` (`#[ignore]`,
  `DRIFT_SAMPLES` env): `10⁹` семплів безперервного рендеру проти
  **Kahan-компенсованого точно-wrapped еталона** (точність `~10⁻¹⁶`
  оберт/крок). Порівнює й несучу, і `fm_phase` (з `floor_f64`-wrap).
- **Результат (5.79 год аудіо).** Відхилення фази росте **лінійно**
  (`2.3·10⁻⁸` обертів несуча / `1.7·10⁻⁸` FM — bias округлення `≈0.1 ulp/семпл`),
  але похибка **частоти** від тривалості не залежить: **`5·10⁻⁹` ppm**
  несуча, `1.3·10⁻⁹` ppm FM — `~10⁻¹²` Гц на 220 Гц. Дев'ять порядків
  запасу до 1 цента. `06 §3` / `07 §7` оновлено, застереження в `06 §7`
  знято.
- **Не робив.** Окремий FFT-аналіз хвоста — прямий фазовий еталон строгіший
  і дешевший; DAW-рівневий 12-год прогін лишається для В2.

---

## Рекомендована послідовність

1. ~~**Б3** (крос-компіляція + bit-exact на ARM)~~ — ✅ виконано (RFC-15:
   4 таргети чисто; 70/70 біт-у-біт на `aarch64` + `armv7-hf` під QEMU).
2. ~~**А1** (PolyBLEP пила/трикутник)~~ — ✅ виконано (плаский бас, ~3× швидше).
3. ~~**Б2** (фікс CLAP-обгортки)~~ — ✅ `[patch]` на `vendor/nih-plug` →
   `clap-validator` 35/35; лишається лише публічний upstream-PR.
4. ~~**Б1** (fast path)~~ — ✅ виконано (+25 % голос / +80 % поліфонія).
5. ~~**А2** (LFO retrigger + матриця)~~ — ✅ виконано.
6. ~~**В3** (drift-тест)~~ — ✅ виконано (`5·10⁻⁹` ppm).
7. ~~**А3** (живий унісон)~~ — ✅ виконано (drift-LFO; мікро-затримка свідомо ні).
8. ~~**В1** (GUI на `nih_plug_vizia`)~~ — ✅ виконано (мінімальний; полиш layout — окремо).
9. **В2** (жива DAW-валідація) — ручна; лишається останнім.

Після В2 всі 9 пунктів роадмапи закриті (з поправкою: Б2 чекає публічного PR,
кілька «полиш»-хвостиків задокументовані в самих пунктах).
