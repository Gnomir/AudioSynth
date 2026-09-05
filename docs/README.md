# Технічна документація: harmonic_core / harmonic_synth

Повний опис DSP-рушія band-limited адитивного синтезу на закритій формі
**ядра Діріхле** та його геометричного узагальнення, разом із поліфонічним
плагіном VST3/CLAP над ним.

Це **не** AQOE-AudioSynth: формула `cos²(2εθ)` та «квантово-натхнена»
маршрутизація тут не використовуються (та ідея була проаналізована й
відхилена — `cos²(2εθ)` як функція `N` вироджена як спектральна обгинаюча).
Тут — інша, чесна архітектура: осцилятор `Σ rᵏ·cos(2πkp)` у закритій формі,
з явним, вимірюваним бюджетом вартості та смуги.

## Технічна документація

| Документ | Зміст |
|---|---|
| [01_MATHEMATICS.md](01_MATHEMATICS.md) | Виведення ядра Діріхле та геометричної суми з комплексного ряду; властивості, точна скінченність, аналіз похибки, О-нотація |
| [02_NUMERICAL_METHODS.md](02_NUMERICAL_METHODS.md) | Тригонометрія `no_std` без `libm`: поліноми Горнера, діапазонна редукція в обертах, `exp2`, `tan`; межі ULP; branchless-батч для SIMD |
| [03_ARCHITECTURE.md](03_ARCHITECTURE.md) | Граф модулів, `Voice` / `PolySynth`, сигнальний тракт, типи даних, контракт RT-safety, матриця збірки, модель володіння FFI |
| [04_DSP_COMPONENTS.md](04_DSP_COMPONENTS.md) | Character-стадія, HQ-режим (Unified HQ Bus), ZDF SVF (повне виведення Cytomic), ADSR, LFO, панорама рівної потужності, де-клік, pitch bend, унісон, soft-clip |
| [05_API_REFERENCE.md](05_API_REFERENCE.md) | Rust API (кожен публічний метод) та C-ABI (кожна експортована функція), одиниці, діапазони клампу, RT vs setup |
| [06_VERIFICATION.md](06_VERIFICATION.md) | Методологія тестування, каталог усіх 70 тестів, виміряні числа, бенчмарки, `pluginval`, крос-верифікація на ARM (bit-exact), що НЕ покрито |
| [07_LIMITATIONS.md](07_LIMITATIONS.md) | Чесні межі: Θ(log n) а не O(1); аліасинг на нелінійних стадіях; стеля 2048 гармонік; один MIDI-канал; педаль сустейну на рівні плагіна; відсутність DAW-валідації тощо |
| [08_EMBEDDED_INTEGRATION.md](08_EMBEDDED_INTEGRATION.md) | Регламент інтеграції в C/C++/RTOS: `no_std`-контракт, макет пам'яті, протокол C-ABI, збірка під ARM/RISC-V, що гарантовано / що ні |
| [09_ROADMAP.md](09_ROADMAP.md) | Що лишилось: активні задачі (жива DAW-валідація, upstream-PR) та свідомо відкладені напрямки з причиною |
| [10_NIH_PLUG_CLAP_BUGS.md](10_NIH_PLUG_CLAP_BUGS.md) | Два баги CLAP-обгортки nih-plug (`ext_state_load`: немає `rescan`; `Vec::with_capacity` на невалідованій довжині → abort); корінь, патч, коли прибрати |

## Практична документація

| Документ | Зміст |
|---|---|
| [12_USER_GUIDE.md](12_USER_GUIDE.md) | Збірка плагіна з коду, куди потрапляє бандл, підключення в DAW (кроки для REAPER), швидкий старт, довідник параметрів, траблшутинг |
| [11_DAW_CHECKLIST.md](11_DAW_CHECKLIST.md) | Протокол ручної валідації в живих DAW (Ableton / Reaper / Bitwig / Logic): ініціалізація, буфери, SR, автоматизація, state recall, кілька інстансів, сустейн-педаль, voice-stealing |

## Порядок читання

- **Оцінити математичну коректність** → 01 → 06.
- **Інтегрувати рушій у свій код** → 03 → 05 → 07.
- **Зрозуміти звукові компоненти** → 04.
- **Портувати на MCU / інший таргет** → 02 → 03 (матриця збірки) → 07.
- **Зібрати й запустити плагін у DAW** → 12 → 11.

## Rustdoc

Кожен публічний елемент має докстрінг. Згенерувати HTML:

```
cd harmonic_core && cargo doc --no-deps --open
```

## Статус

70 тестів проходять (65 юніт + 5 інтеграційних) + 1 `#[ignore]` (дрейф),
біт-у-біт на `aarch64` + `armv7-hf` під QEMU (`cross-verify.sh`) · clippy
чистий (stable + `--no-default-features --release` + nightly
`--features portable-simd`) · плагін збирається у VST3 + CLAP, має GUI
(`nih_plug_vizia`: усі параметри + живий спектр) · **pluginval
`--strictness-level 8`: повний прохід (VST3, з GUI-тестами)** ·
**clap-validator: 35/35** (потребує `[patch]` на `vendor/nih-plug`,
`10_NIH_PLUG_CLAP_BUGS.md`) · осцилятор: closed-form additive + PolyBLEP
пила/трикутник (плаский до DC) · clean-voice fast path · LFO: retrigger/
free-run + матриця (→ brightness / pitch / cutoff / FM index) · унісон з
«диханням» · CC#64 sustain (рівень плагіна) · `panic=abort` в обох
крейтах.
