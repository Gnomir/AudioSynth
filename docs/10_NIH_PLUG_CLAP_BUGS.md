# 10 — Баги CLAP-обгортки nih-plug (Roadmap Б2)

Два дефекти у `src/wrapper/clap/wrapper.rs` фреймворку **nih-plug**, через які
`harmonic_synth.clap` не проходить 4 тести `clap-validator`. VST3-шлях того
самого стану проходить `pluginval --strictness 8` повністю — тобто це саме
CLAP-обгортка, не наш плагін.

- **Виявлено на** pinned rev `de421011f41a6d10fc8c7a6084e4f4dee0143683`.
- **Підтверджено на** `master` (вересень 2026) — код `ext_state_load`
  ідентичний, обидва баги присутні.
- **Статус: фікс застосовано локально.** Патчена копія pinned-дерева лежить у
  `harmonic_synth/vendor/nih-plug/`, підключена через `[patch]` у
  `harmonic_synth/Cargo.toml`. `clap-validator` — **35 / 35, 0 failed,
  0 warnings**; `--exclude` зі скриптів `validate.{ps1,sh}` **прибрано**.
  Публічний upstream-PR — за користувачем (`Gnomir`); після мержу: прибрати
  `vendor/nih-plug` + секцію `[patch]`, бампнути `rev`.
- **Що у `vendor/nih-plug`:** тільки `nih_plug`, `nih_plug_derive`,
  `nih_plug_vizia`, `nih_plug_xtask` (без `plugins/`, `nih_plug_egui`,
  `nih_plug_iced`, `cargo_nih_plug`, `xtask`, `.git`). Зміни від upstream
  `de421011`: (1) патч нижче у `src/wrapper/clap/wrapper.rs`; (2) два
  `#[allow]` для чужих ворнінгів (`unused_imports`, `mismatched_lifetime_syntaxes`);
  (3) вирізаний `[workspace]` з кореневого `Cargo.toml`. Більше нічого.

---

## Баг 1 — після `clap_plugin_state::load()` хост не отримує `rescan`

### Симптом (`clap-validator`)

```
state-reproducibility-basic     FAILED
state-reproducibility-binary    FAILED
state-reproducibility-buffered  FAILED

  After reloading the state, these parameter values changed
  without a rescan request:
   - Fold (3148801) - 0 (0.0000) vs 6 (0.0556)
   - Gain (3165055) - -12.0 dB (0.7593) vs -58.5 dB (0.1821)
   - Grit (3181398) - 0 (0.0000) vs 55 (0.5452)
   ...and 24 more
```

### Корінь

`ext_state_load` (`wrapper.rs`, ~рядок 3138) викликає `wrapper.set_state_inner(&mut state)`.
`set_state_inner` постить лише `Task::ParameterValuesChanged`, а його обробник —

```rust
Task::ParameterValuesChanged => {
    if self.editor_handle.lock().is_some() {
        if let Some(editor) = self.editor.borrow().as_ref() {
            editor.lock().param_values_changed();   // ← лише GUI самого плагіна
        }
    }
}
```

— повідомляє **тільки редактор самого плагіна**. Хост, який щойно викликав
`clap_plugin_state::load()`, не отримує `clap_host_params::rescan(CLAP_PARAM_RESCAN_VALUES)`
і показує старі значення параметрів.

Правильний шлях уже є в тому ж файлі: `set_state_object_from_gui` (завантаження
пресета з GUI плагіна) після `set_state_inner` **додатково** постить
`Task::RescanParamValues`, чий обробник викликає
`host_params=>rescan(&*self.host_callback, CLAP_PARAM_RESCAN_VALUES)`.
`ext_state_load` цього кроку не робить.

VST3-обгортка технічно має ту саму структуру (`set_state_inner` теж лише
`Task::ParameterValuesChanged`), але VST3-хости після `IComponent::setState`
самі перечитують параметри через контролер, тому `pluginval` не скаржиться.

### Фікс

Після успішного `set_state_inner` в `ext_state_load` — постити
`Task::RescanParamValues` (аналогічно `set_state_object_from_gui`). `state.load`
у CLAP — `[main-thread]`, тож `schedule_gui` виконає його синхронно.

---

## Баг 2 — `Vec::with_capacity` на невалідованій довжині зі стріму → abort

### Симптом (`clap-validator`)

```
state-invalid-random  CRASHED  (exit code: 0xc0000409)
  memory allocation of 1025222176999353387 bytes failed
```

`0xc0000409` = `STATUS_STACK_BUFFER_OVERRUN` — так виглядає Rust-івський
`abort()` на Windows. Тест `state-invalid-random` вантажить 3×1 МБ випадкових
байтів через `clap_plugin_state::load()` і очікує, що плагін **не впаде**.

### Корінь

```rust
let length = u64::from_le_bytes(length_bytes);          // ← перші 8 байтів стріму
let mut read_buffer: Vec<u8> = Vec::with_capacity(length as usize);   // ← BOOM
```

`length` — це префікс, який nih-plug сам додає перед JSON-стейтом. На
пошкодженому / випадковому стрімі це сміття (в середньому ~2⁶³). `Vec::with_capacity`
на такому значенні → невдала алокація → `handle_alloc_error` → `abort()` →
краш хоста. Той самий патерн у VST3-обгортці (`stream_byte_size`), але там
розмір бере хост через `IBStream::seek(kIBSeekEnd)`, а не з даних, тож він
надійніший.

### Фікс

`try_reserve_exact(length as usize)` замість `Vec::with_capacity` — невдала
алокація стає `Err`, який повертається як `false` (невдале завантаження), а не
abort. Жодного довільного ліміту не треба: якщо `length` чесний, але завеликий
для доступної памʼяті — теж коректно повертаємо `false`.

---

## Патч (проти `de421011`, застосовується і на `master`)

```diff
--- a/src/wrapper/clap/wrapper.rs
+++ b/src/wrapper/clap/wrapper.rs
@@ -3153,7 +3153,19 @@ impl<P: ClapPlugin> Wrapper<P> {
         }
         let length = u64::from_le_bytes(length_bytes);

-        let mut read_buffer: Vec<u8> = Vec::with_capacity(length as usize);
+        // `length` comes straight from the stream and cannot be trusted. A corrupt
+        // or truncated preset (clap-validator's `state-invalid-random` loads pure
+        // random bytes) yields a garbage value here, and `Vec::with_capacity()`
+        // aborts the entire process when that allocation fails. `try_reserve_exact`
+        // turns that into a recoverable error instead.
+        let mut read_buffer: Vec<u8> = Vec::new();
+        if read_buffer.try_reserve_exact(length as usize).is_err() {
+            nih_debug_assert_failure!(
+                "Could not allocate {} bytes for the state buffer",
+                length
+            );
+            return false;
+        }
         if !read_stream(&*stream, read_buffer.spare_capacity_mut()) {
             nih_debug_assert_failure!(
                 "Error or end of stream while reading the state buffer from the stream."
@@ -3167,6 +3179,16 @@ impl<P: ClapPlugin> Wrapper<P> {
                 let success = wrapper.set_state_inner(&mut state);
                 if success {
                     nih_trace!("Loaded state ({} bytes)", read_buffer.len());
+
+                    // `set_state_inner()` only notifies the plugin's own editor
+                    // through `Task::ParameterValuesChanged`. The host that called
+                    // `clap_plugin_state::load()` also needs to be told that the
+                    // parameter values changed, the same way `set_state_object_from_gui()`
+                    // does after a preset load from the plugin's GUI. Without this a
+                    // CLAP host keeps showing the pre-load values until the next
+                    // rescan (clap-validator's `state-reproducibility-*`).
+                    let task_posted = wrapper.schedule_gui(Task::RescanParamValues);
+                    nih_debug_assert!(task_posted, "The task queue is full, dropping task...");
                 }

                 success
```

`try_reserve_exact` — стабільний з Rust 1.63; nih-plug MSRV `1.80`, тож
без проблем.

---

## Верифікація

`vendor/nih-plug` (pinned `de421011` + патч) підключено через `[patch]`,
`--exclude` зі скриптів прибрано, `cargo xtask validate`:

| | до | після |
|---|---|---|
| `clap-validator` | 31 passed, **4 failed/crashed**, 9 skipped | **35 passed, 0 failed, 0 warnings**, 9 skipped |
| `pluginval --strictness 8` (VST3, з GUI-тестами) | SUCCESS | SUCCESS (не зачеплено) |
| `harmonic_core` тести | 57/57 | 57/57 (не залежать від nih-plug) |

Регресійного тесту на це немає — воно у скрипті валідації, який не в
`cargo test` (потребує зовнішніх бінарників). `06_VERIFICATION.md §6` фіксує
очікуваний результат.

---

## Що робити далі

1. **Подати upstream (за користувачем).** Форк `robbert-vdh/nih-plug` (напр.
   `Gnomir/nih-plug`) → гілка → PR з цим текстом і патчем
   `contrib/nih-plug-clap-state-load-fix.patch`.
2. **Після мержу upstream:** бампнути pinned `rev` у
   `harmonic_synth/Cargo.toml` + `xtask/Cargo.toml`, видалити
   `harmonic_synth/vendor/nih-plug/` та секцію `[patch]`, прибрати рядок
   `harmonic_synth/vendor/**` з `.gitattributes`. `clap-validator` має
   лишитися 35/35. Оновити цей файл, `06 §6`, `09` (пункт «Б2»).
