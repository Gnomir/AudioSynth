# Технічна документація: harmonic_core / harmonic_synth

Повний опис DSP-рушія band-limited адитивного синтезу на закритій формі
**ядра Діріхле** та його геометричного узагальнення, разом із поліфонічним
плагіном VST3/CLAP над ним.

Це **не** AQOE-AudioSynth: формула `cos²(2εθ)` та «квантово-натхнена»
маршрутизація тут не використовуються (та ідея була проаналізована й
відхилена — `cos²(2εθ)` як функція `N` вироджена як спектральна обгинаюча).
Тут — інша, чесна архітектура: осцилятор `Σ rᵏ·cos(2πkp)` у закритій формі,
з явним, вимірюваним бюджетом вартості та смуги.

## Склад

| Документ | Зміст |
|---|---|
| [01_MATHEMATICS.md](01_MATHEMATICS.md) | Виведення ядра Діріхле та геометричної суми з комплексного ряду; властивості, точна скінченність, аналіз похибки, О-нотація |
| [02_NUMERICAL_METHODS.md](02_NUMERICAL_METHODS.md) | Тригонометрія `no_std` без `libm`: поліноми Горнера, діапазонна редукція в обертах, `exp2`, `tan`; межі ULP; branchless-батч для SIMD |
| [03_ARCHITECTURE.md](03_ARCHITECTURE.md) | Граф модулів, `Voice` / `PolySynth`, сигнальний тракт, типи даних, контракт RT-safety, матриця збірки, модель володіння FFI |
| [04_DSP_COMPONENTS.md](04_DSP_COMPONENTS.md) | Character-стадія, ZDF SVF (повне виведення Cytomic), ADSR, LFO, панорама рівної потужності, де-клік, pitch bend, унісон, soft-clip |
| [05_API_REFERENCE.md](05_API_REFERENCE.md) | Rust API (кожен публічний метод) та C-ABI (кожна експортована функція), одиниці, діапазони клампу, RT vs setup |
| [06_VERIFICATION.md](06_VERIFICATION.md) | Методологія тестування, каталог усіх 70 тестів, виміряні числа, бенчмарки, `pluginval`, крос-верифікація на ARM (bit-exact), що НЕ покрито |
| [07_LIMITATIONS.md](07_LIMITATIONS.md) | Чесні межі: Θ(log n) а не O(1); аліасинг на нелінійних стадіях; стеля 2048 гармонік; відсутність DAW-валідації тощо |
| [08_EMBEDDED_INTEGRATION.md](08_EMBEDDED_INTEGRATION.md) | Регламент інтеграції в C/C++/RTOS: `no_std`-контракт, точний макет пам'яті (520 б, align 8), протокол C-ABI, збірка під ARM/RISC-V, що гарантовано / що ні |
| [09_ROADMAP.md](09_ROADMAP.md) | Дорожня карта: 3 вектори (DSP-розширення · оптимізація/портування · GUI/QA), кожен пункт з Проблема/Рішення/Файли/Оцінка/DoD; рекомендована послідовність |
| [10_NIH_PLUG_CLAP_BUGS.md](10_NIH_PLUG_CLAP_BUGS.md) | Два баги CLAP-обгортки nih-plug (`ext_state_load`: немає `rescan`; `Vec::with_capacity` на невалідованій довжині → abort); корінь, патч, верифікація 35/35. Roadmap Б2 |
| [11_DAW_CHECKLIST.md](11_DAW_CHECKLIST.md) | Протокол ручної валідації в живих DAW (Ableton / Reaper / Bitwig / Logic): тест-кейси ініціалізації, буферів, SR, автоматизації, state recall, кілька інстансів. Roadmap В2 |
| [12_TECHNICAL_SPEC_RFC.md](12_TECHNICAL_SPEC_RFC.md) | RFC технічного директора: `tanh_pade` кламп `4→3` (усунення C¹-зламу), клік S&H (спростовано), `tan_turns_fast` рац. мінімакс (SVF `2.1×`), `exp2` Remez мінімакс (`×70` точніше). Статус: закрито |
| [13_RFC_SIMD_OVERSAMPLING_ARM.md](13_RFC_SIMD_OVERSAMPLING_ARM.md) | RFC-13/14/15: SoA-SIMD (відкладено — немає CPU-тиску), оверсемплінг майстра (частково — 2× soft_clip так, оверсемпл лінійного SVF ні), **портативна bit-exact верифікація на ARM (✅ зроблено, дельта = 0.0)** |
| [14_RFC_AUDIT_HQ_BUS_TAN_MXCSR.md](14_RFC_AUDIT_HQ_BUS_TAN_MXCSR.md) | RFC-16/17 «5 прихованих компромісів», перевірено проти коду: `tan_turns_fast`-полюс і MXCSR-крихкість спростовано як живі баги (захисний кламп + документація додані), minimum-phase FIR відхилено, PolyBLEP-шейв відкладено, Unified HQ Bus — правильна архітектура (реалізація → `15`) |
| [15_TECHNICAL_SPEC_HQ_BUS.md](15_TECHNICAL_SPEC_HQ_BUS.md) | RFC-16 Unified HQ Bus: реалізовано. Аліасинг майстра `−94.6` дБ (DoD `−75`) ✅; CPU-DoD `40–50 %` ❌ спростовано вимірюванням (`+0.5…+2.7 %` на 12/16/24 голосах — дециматор ніколи не був вузьким місцем). Стандалон `Voice`/C-ABI HQ-шлях лишився недоторканим |
| [16_RFC19_AUDIT_DOCS_DC_SH_NOSTD.md](16_RFC19_AUDIT_DOCS_DC_SH_NOSTD.md) | RFC-19 аудит: `.min()` у `no_std` — спростовано з доказом (`core`-safe, список заборонених викликів виправлено); 2 реальних DSP-баги виправлено (DC-blocker rate-scaling, S&H near-zero jitter, обидва з регресійними тестами); PolyBLEP ZOH-пастка — спростована вимірюванням; ADSR/SVF rate «проблема» — спростована; латентність/wording у 04/05/13 виправлено |
| [17_RFC_AUDIT_STATE_CLAMP_UNISON.md](17_RFC_AUDIT_STATE_CLAMP_UNISON.md) | Наступний аудит: HQ cutoff-стеля `0.45×2fs` vs `0.45×fs` — реальний баг, виправлено (`Svf::base_sample_rate`); DC-blocker reset — переспростовано (той самий хибний закид, що й RFC-12 §1.2); S&H bypass-freeze — залишено з обґрунтуванням; унісон при `detune=0` — спростовано вимірюванням: гасить (аж до тиші), не перевантажує |
| [18_RFC_AUDIT_DRIFT_SR_STEAL_COMB.md](18_RFC_AUDIT_DRIFT_SR_STEAL_COMB.md) | Гребінчастий фільтр унісону — точна математика додана (`07 §13`); `drift_phase` необмежений ріст і `sample_rate=0` при ігноруванні `initialize→false` — обидва **хибні**, код уже вирішує це (wrap існує; `validate_sample_rate` унеможливлює 0/NaN; плагін завжди повертає `true`); клацання при voice-stealing — реальна, задокументована (не виправлена) плата за компактний `Voice` |
| [19_INDEPENDENT_AUDIT_REPORT.md](19_INDEPENDENT_AUDIT_REPORT.md) | Формальний незалежний аудит (повний шаблон: критичні проблеми, математичні невідповідності, сигнальний тракт, вимоги, відсутні дані). Знайдено й виправлено: `NaN`-проходження крізь клампи параметрів на межі C-ABI (кат. B), непровірене переповнення в `harmonic_voice_process` (кат. B), дубльована математика `soft_clip`/`tanh_pade` без симетричних тестів (кат. C). Жодної критичної (кат. A) проблеми |

## Порядок читання

- **Оцінити математичну коректність** → 01 → 06.
- **Інтегрувати рушій у свій код** → 03 → 05 → 07.
- **Зрозуміти звукові компоненти** → 04.
- **Портувати на MCU / інший таргет** → 02 → 03 (матриця збірки) → 07.

## Rustdoc

Кожен публічний елемент має докстрінг. Згенерувати HTML:

```
cd harmonic_core && cargo doc --no-deps --open
```

## Статус (2026-09-04)

70 тестів проходять (65 юніт + 5 інтеграційних) + 1 `#[ignore]` (дрейф),
біт-у-біт на `aarch64` + `armv7-hf` під QEMU (`cross-verify.sh`) · clippy чистий (stable +
`--no-default-features --release` + nightly `--features portable-simd`) ·
плагін збирається у VST3 + CLAP, має GUI (`nih_plug_vizia`: усі параметри +
живий спектр) · **pluginval `--strictness-level 8`: повний прохід (VST3, з
GUI-тестами)** · **clap-validator: 35/35** (баг `ext_state_load` виправлено
через `[patch]` на `vendor/nih-plug`, `10_NIH_PLUG_CLAP_BUGS.md`) ·
осцилятор: closed-form additive + PolyBLEP пила/трикутник (плаский до DC) ·
clean-voice fast path (~+25 % на голос, ~+80 % поліфонії) · LFO: retrigger/
free-run + матриця (→ brightness / pitch / cutoff / FM index) · унісон з «диханням».
