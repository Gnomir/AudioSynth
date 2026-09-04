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
| [06_VERIFICATION.md](06_VERIFICATION.md) | Методологія тестування, каталог усіх 58 тестів, виміряні числа, бенчмарки, `pluginval`, що НЕ покрито |
| [07_LIMITATIONS.md](07_LIMITATIONS.md) | Чесні межі: Θ(log n) а не O(1); аліасинг на нелінійних стадіях; стеля 2048 гармонік; відсутність DAW-валідації тощо |
| [08_EMBEDDED_INTEGRATION.md](08_EMBEDDED_INTEGRATION.md) | Регламент інтеграції в C/C++/RTOS: `no_std`-контракт, точний макет пам'яті (512 б, align 8), протокол C-ABI, збірка під ARM/RISC-V, що гарантовано / що ні |
| [09_ROADMAP.md](09_ROADMAP.md) | Дорожня карта: 3 вектори (DSP-розширення · оптимізація/портування · GUI/QA), кожен пункт з Проблема/Рішення/Файли/Оцінка/DoD; рекомендована послідовність |
| [10_NIH_PLUG_CLAP_BUGS.md](10_NIH_PLUG_CLAP_BUGS.md) | Два баги CLAP-обгортки nih-plug (`ext_state_load`: немає `rescan`; `Vec::with_capacity` на невалідованій довжині → abort); корінь, патч, верифікація 35/35. Roadmap Б2 |

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

58 тестів проходять (54 юніт + 4 інтеграційні) + 1 `#[ignore]` (дрейф) · clippy чистий (stable +
`--no-default-features --release` + nightly `--features portable-simd`) ·
плагін збирається у VST3 + CLAP, має GUI (`nih_plug_vizia`: усі параметри +
живий спектр) · **pluginval `--strictness-level 8`: повний прохід (VST3, з
GUI-тестами)** · **clap-validator: 35/35** (баг `ext_state_load` виправлено
через `[patch]` на `vendor/nih-plug`, `10_NIH_PLUG_CLAP_BUGS.md`) ·
осцилятор: closed-form additive + PolyBLEP пила/трикутник (плаский до DC) ·
clean-voice fast path (~+25 % на голос, ~+80 % поліфонії) · LFO: retrigger/
free-run + матриця (→ brightness / pitch / cutoff / FM index) · унісон з «диханням».
