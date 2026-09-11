// One-time content pass: rewrites the persuasive copy (hero, differentiators,
// feature tour, pricing framing, FAQ, published limitations, prior-art table)
// from DSP/CS language into language a working musician reads and values —
// sound, reliability, "won't wreck my set" — while leaving the "For
// developers" section technical on purpose (it's a different, labeled
// audience: engine licensees). Run: node scripts/rewrite-musician-copy.js
'use strict';

const { setValue } = require('../src/lib/content');
const { db } = require('../src/db');

// key -> { en, uk, section } — is_html inferred from existing row, kept as-is.
const BLOCKS = {
  'brand.prov': { en: 'additive synth', uk: 'адитивний синт' },
  'hero.eyebrow': { en: 'Additive synth · built to never let you down', uk: 'Адитивний синт · створений, щоб не підвести' },
  'hero.h1': { en: '<em>Cosine.</em> One knob, endless brightness — and it never breaks a sweat doing it.', uk: '<em>Cosine.</em> Одна ручка — безмежна яскравість, і жодного зайвого навантаження.' },
  'hero.sub': {
    en: 'Turn Brightness up and the tone goes from a pure, dark sine to a buzzing wall of harmonics — smoothly, with <strong>no extra CPU cost</strong> no matter how many voices are stacked on top. It’s a real, deterministic engine under the hood — <strong>a freeze or bounce sounds identical months later, on any machine</strong> — sold as a finished VST3 / CLAP plugin.',
    uk: 'Крути Яскравість — і тон іде від чистого темного синуса до дзижчачої стіни гармонік, плавно, без <strong>жодних додаткових витрат CPU</strong>, скільки б голосів не було накладено. Під капотом — справжній детермінований рушій: <strong>фриз чи зведення звучить так само й через кілька місяців, на будь-якій машині</strong> — продається як готовий плагін VST3 / CLAP.',
  },
  'hero.cta.demo': { en: '▶ Try it in your browser', uk: '▶ Спробувати в браузері' },

  'why.kick': { en: 'Why musicians pick it', uk: 'Чому музиканти обирають це' },
  'why.1.h': { en: 'One knob, zero CPU tax', uk: 'Одна ручка, нуль податку на CPU' },
  'why.1.p': {
    en: 'Brightness sweeps your tone from dark and round to bright and buzzing. Most synths get more expensive to run the richer the sound gets — Cosine <strong>doesn’t</strong>. Stack full chords, load a fat unison, push the harmonic content as far as it goes — your CPU meter barely moves.',
    uk: 'Яскравість веде тон від темного й округлого до яскравого й дзвінкого. У більшості синтів чим багатший звук — тим важче для процесора. У Cosine <strong>ні</strong>. Тримай повні акорди, товстий унісон, максимум гармонік — індикатор CPU майже не рухається.',
  },
  'why.2.h': { en: 'What you hear is what you keep', uk: 'Що чуєш — те й лишається' },
  'why.2.p': {
    en: 'Freeze a track, bounce it, open the project again next month, mix on a different laptop — Cosine sounds <strong>exactly the same, every time</strong>. No “it sounded different after I rendered it,” ever.',
    uk: 'Заморозь трек, зведи його, відкрий проєкт через місяць, змішуй на іншому ноутбуку — Cosine звучить <strong>абсолютно однаково щоразу</strong>. Ніяких «а чого воно інакше звучить після рендеру».',
  },
  'why.3.h': { en: 'It warns you before your ears do', uk: 'Попереджає раніше за твої вуха' },
  'why.3.p': {
    en: 'The built-in display draws the real shape of your patch’s tone, and an <strong>honest little meter</strong> lights up the moment a sound is about to turn harsh or digital — so you always know exactly when to reach for the cleanup switch.',
    uk: 'Вбудований дисплей малює справжню форму тону твого патча, а <strong>чесний індикатор</strong> загоряється в момент, коли звук ось-ось стане різким чи цифровим — тож ти завжди знаєш, коли тягнутись за кнопкою «почистити».',
  },

  'how.h': { en: 'You don’t need to understand any of this to use Cosine. Here it is anyway, for the curious.', uk: 'Щоб користуватись Cosine, це знати не обов’язково. Але ось воно — для допитливих.' },
  'how.p1': {
    en: 'Under the hood, every note is built by adding up its harmonics directly — the way a pipe organ or a Hammond does it, not a wavetable and not a stack of separate oscillators. That’s the whole trick: turning up Brightness just changes how loud each harmonic is, all at once, for free.',
    uk: 'Під капотом кожна нота будується прямим додаванням її гармонік — так, як це робить орган чи Hammond, а не вейвтейбл і не стос окремих осциляторів. У цьому весь фокус: збільшення Яскравості просто міняє гучність кожної гармоніки одразу, безкоштовно.',
  },
  'how.p2': {
    en: 'Written out, that’s a closed-form sum of cosines — one per harmonic, with a mathematically exact cutoff, so nothing lives above the harmonic you set. That’s the whole reason the oscillator itself can never sound harsh or aliased, no matter how bright you push it:',
    uk: 'Записано формулою, це — замкнена сума косинусів, по одній на гармоніку, з математично точною стелею, тож вище заданої гармоніки нема нічого. Саме тому сам осцилятор ніколи не звучить різко чи «цифрово», хоч як б яскраво його ні викрути:',
  },
  'how.p3': {
    en: 'The one part of that sum that takes any real work stays cheap even at 2000 harmonics, and it’s remembered for as long as you hold a note — which is why the cost never climbs. Full derivation and every measured number, if you want to go deeper: <a href="https://claude.ai/code/artifact/c4b2806f-90f3-4eb9-84c2-35b43461c30d" target="_blank" rel="noopener">the scientific monograph</a>.',
    uk: 'Єдина частина цієї суми, що потребує реальної роботи, лишається дешевою навіть при 2000 гармоніках, і вона запам’ятовується, поки нота тримається, — тому вартість ніколи не росте. Повне виведення й усі виміряні числа, якщо хочеться глибше: <a href="https://claude.ai/code/artifact/c4b2806f-90f3-4eb9-84c2-35b43461c30d" target="_blank" rel="noopener">наукова монографія</a>.',
  },

  'feat.h': { en: 'A 24-voice synth, laid out the way you actually build a sound.', uk: '24-голосний синт, побудований так, як ти реально створюєш звук.' },
  'feat.osc.p': {
    en: 'Brightness sweeps dark-to-bright with zero stepping. Partials caps how many harmonics you hear — great for melting a buzzy tone down into something round and simple, live. Formant adds a resonant, almost vocal peak. Plus band-limited Saw and Triangle for classic analog-style tones.',
    uk: 'Яскравість веде від темного до яскравого без жодних сходинок. Гармоніки обмежують, скільки обертонів чути, — зручно, щоб на льоту перетворити дзижчачий тон на округлий і простий. Формант додає резонансний, майже вокальний пік. Плюс смугово-обмежені Пилка й Трикутник для класичних аналогових тонів.',
  },
  'feat.char.p': {
    en: 'Drive, an asymmetric wavefolder, bit-crush and sample-rate reduction. Silky clean at zero, PPG / DX7-style digital grit the moment you push it.',
    uk: 'Драйв, асиметричний вейвфолдер, біт-крашер і зниження частоти дискретизації. Ідеально чисто на нулі, цифровий грит у стилі PPG / DX7, щойно додаси.',
  },
  'feat.fm.p': {
    en: 'A classic sine-operator FM voice — ratio and amount — plus operator feedback: bell and metallic tones, or push feedback into full-on noise.',
    uk: 'Класичний FM-оператор на синусі — відношення й глибина — плюс зворотний зв’язок оператора: дзвіночкові й металічні тони, або зворотний зв’язок на повну — в шум.',
  },
  'feat.filt.p': {
    en: 'A resonant filter (low/band/high-pass, notch) plus a filter envelope that’s <em>completely independent</em> from the amp envelope — a pluck and its filter sweep can move on their own separate clocks. Envelope times are honest: set 1&nbsp;second, get 1&nbsp;second.',
    uk: 'Резонансний фільтр (LP/BP/HP, нотч) плюс обвідна фільтра, <em>повністю незалежна</em> від амплітудної обвідної — щипок і його зміна фільтра можуть жити за різним годинником. Часи обвідних чесні: постав 1&nbsp;с — отримаєш 1&nbsp;с.',
  },
  'feat.voice.p': {
    en: '24-voice polyphony, and up to 8 voices stacked per note with detune, stereo spread and a slow, natural drift — so big unison pads <strong>breathe</strong> instead of sitting there static. Flip on HQ mode any time a patch needs extra headroom, for a small, fixed amount of latency.',
    uk: '24-голосна поліфонія і до 8 голосів в унісоні на ноту з розстройкою, стерео-розкидом і повільним природним дрейфом — тож товсті унісонні пади <strong>дихають</strong>, а не стоять статично. Вмикай HQ-режим, коли патчу треба більше запасу, за невелику фіксовану затримку.',
  },
  'feat.tune.p': {
    en: 'Just intonation, historical tunings, 19/24/31-tone equal temperament, Bohlen-Pierce, or your own Scala file. Because the harmonics sit exactly where the maths says they should, a just-intonation chord actually <strong>stops beating</strong> — you can hear it lock in. 12-TET stays the untouched default.',
    uk: 'Чиста інтонація, історичні темперації, 19/24/31-щаблева рівна темперація, Бален-Пірс або власний файл Scala. Оскільки гармоніки стоять точно там, де каже математика, акорд у чистій інтонації справді <strong>перестає битися</strong> — це чутно. 12-TET лишається дефолтом без змін.',
  },
  'feat.expr.p': {
    en: 'Press one note of a chord harder — MPE, poly aftertouch, channel pressure — and only that note gets brighter. Real per-note expression, not a global filter sweep.',
    uk: 'Натисни одну ноту акорду сильніше — MPE, поліфонічний post-touch, канальний тиск — і яскравішою стане лише вона. Справжня понотна експресія, а не загальний свіп фільтра.',
  },
  'feat.mod.p': {
    en: 'A per-voice LFO — sine, triangle or saw, retriggered per note or running free — that can hit brightness, vibrato, filter cutoff or FM depth. Pick one target, or stack a few at once.',
    uk: 'Понотний LFO — синус, трикутник чи пилка, ретригер на кожну ноту або вільний хід — б’є по яскравості, вібрато, зрізу фільтра чи глибині FM. Обери одну ціль або накладай кілька разом.',
  },
  'feat.work.p': {
    en: 'A live A/B morph slider you can automate for real. A 6-character code that reproduces any random patch exactly — share a sound in a text message instead of a preset file. 22 starting presets to build from.',
    uk: 'Живий A/B-морф-повзунок, який можна по-справжньому автоматизувати. 6-символьний код, що точно відтворює будь-який випадковий патч — ділись звуком у повідомленні, а не файлом пресета. 22 стартові пресети для старту.',
  },
  'feat.see.p': {
    en: 'Watch the real shape of your tone on screen — the harmonic comb and the filter curve, both moving live with your modulation — plus a simple meter that lights up the moment something’s about to sound harsh, so you know exactly when to reach for the cleanup switch.',
    uk: 'Дивись справжню форму свого тону на екрані — гребінка гармонік і крива фільтра, обидві рухаються наживо з модуляцією — плюс простий індикатор, що загоряється, коли щось ось-ось зазвучить різко, тож ти точно знаєш, коли тягнутись за «почистити».',
  },

  'price.foot': {
    en: 'One binary — Studio unlocks with a personal key file that just works: no login, no internet check, no nagging, ever.',
    uk: 'Один бінарник — Studio відмикається персональним ключ-файлом, який просто працює: без логіну, без перевірки в інтернеті, без нагадувань, ніколи.',
  },

  'final.sub': {
    en: 'A real free tier, a paid Studio tier at an honest price, and a 3-minute demo that’s the whole pitch: one Brightness sweep, Drive pushed till the meter goes red, HQ flipped on, watch it clean up.',
    uk: 'Справжній безкоштовний рівень, платний Studio за чесною ціною і 3-хвилинне демо, що і є весь пітч: одне проведення Яскравості, Drive до червоного індикатора, вмикаємо HQ — дивись, як усе очищується.',
  },

  'lim.1': {
    en: '<strong>It’s not infinitely flexible.</strong> One knob controls the overall brightness tilt, not each harmonic by hand. If you need to hand-draw a completely custom, wild harmonic curve partial-by-partial, that’s a different (and much slower) kind of synth.',
    uk: '<strong>Це не безмежно гнучко.</strong> Одна ручка керує загальним нахилом яскравості, а не кожною гармонікою окремо вручну. Якщо треба намалювати вручну повністю довільну, дику криву гармонік по-парціально — це вже інший (і набагато повільніший) тип синта.',
  },
  'lim.2': {
    en: '<strong>The oscillator can’t sound harsh; the effects can.</strong> Drive, Fold, Grit, FM and feedback generate real digital edge on purpose when you push them — that’s intentional character. HQ Mode cleans it up for a small, fixed amount of latency.',
    uk: '<strong>Осцилятор не може звучати різко; ефекти — можуть.</strong> Drive, Fold, Grit, FM і зворотний зв’язок навмисно дають справжню цифрову різкість, коли їх викручуєш, — це навмисний характер. HQ-режим чистить це за невелику фіксовану затримку.',
  },
  'lim.3': {
    en: '<strong>HQ Mode won’t lower your CPU usage.</strong> It’s a cleanup switch for the dirt/FM stages, not a performance mode. We say this everywhere so nobody’s surprised.',
    uk: '<strong>HQ-режим не знизить навантаження CPU.</strong> Це перемикач очищення для брудних стадій/FM, а не режим продуктивності. Ми кажемо це всюди, щоб нікого це не дивувало.',
  },
  'lim.4': {
    en: '<strong>Microtuning retunes the fundamental, not each overtone.</strong> Just intonation, historical and equal-division scales all work; true bell inharmonicity or a stretched-piano tuning need a different kind of oscillator.',
    uk: '<strong>Мікротюнінг ретюнить фундаментал, не кожен обертон окремо.</strong> Чиста інтонація, історичні й рівноподільні шкали працюють; справжня інгармонічність дзвону чи розтягнутий стрій піаніно потребують іншого типу осцилятора.',
  },
  'lim.5': {
    en: '<strong>One MIDI channel at a time</strong>, a 2048-harmonic ceiling, and a small click if you steal past the 24-voice limit.',
    uk: '<strong>Один MIDI-канал за раз</strong>, стеля 2048 гармонік, і легке клацання, якщо крадеш голос понад ліміт у 24.',
  },
  'lim.6': {
    en: '<strong>Real embedded hardware hasn’t been measured yet</strong> — it compiles clean and passes every check we can run without a physical board. macOS / Linux builds and an AU version are still on the way.',
    uk: '<strong>Реальне вбудоване залізо ще не виміряне</strong> — компілюється чисто й проходить усі перевірки, які можна зробити без фізичної плати. Білди macOS / Linux і версія AU — ще в дорозі.',
  },

  'cmp.r1b': {
    en: 'Full manual control over every harmonic, but it gets slower the richer the sound gets and needs pre-baked anti-aliased tables. Cosine: one knob, flat cost, guaranteed clean — the trade is you get a tilt + one hump, not total per-harmonic control.',
    uk: 'Повний ручний контроль кожної гармоніки, але чим багатший звук — тим повільніше, і потрібні заготовлені антиаліасні таблиці. Cosine: одна ручка, стала вартість, гарантована чистота — компроміс у тому, що це нахил + один горб, а не повний контроль кожної гармоніки.',
  },
  'cmp.r2b': {
    en: 'Same family of maths. Cosine adds the Brightness knob as a real-time control, the fractional Partials ceiling, the Formant hump, and a guarantee it sounds the same on every machine.',
    uk: 'Та сама математична родина. Cosine додає ручку Яскравості як контроль у реальному часі, дробову стелю Гармонік, горб Форманта і гарантію однакового звучання на будь-якій машині.',
  },
  'cmp.r3b': {
    en: 'Great for classic analog waveforms — and Cosine <em>includes</em> those (Saw/Triangle), plus the additive engine for tones a subtractive synth simply can’t reach.',
    uk: 'Чудові для класичних аналогових форм — і Cosine <em>включає</em> їх (Пилка/Трикутник), плюс адитивний рушій для тонів, недосяжних субтрактивному синту.',
  },
  'cmp.r4b': {
    en: 'The deliberate opposite: every claim here is measured, and the limitations are published on this very page.',
    uk: 'Навмисна протилежність: кожне твердження тут виміряне, а обмеження опубліковані прямо на цій сторінці.',
  },

  'spec.k3': { en: 'identical sound on every chip — x86 · ARM · in your browser', uk: 'однаковий звук на будь-якому чіпі — x86 · ARM · у браузері' },
  'spec.k4': { en: 'how clean HQ mode gets (lower is cleaner)', uk: 'наскільки чистим HQ-режим (нижче — чистіше)' },
  'spec.k6': { en: 'automated tests that must pass before any update ships', uk: 'автоматичні тести, які мають пройти перед будь-яким оновленням' },
  'spec.k7': { en: 'passes every official plugin-format check', uk: 'проходить усі офіційні перевірки формату плагіна' },
  'spec.k8': { en: 'engine size — small enough for a $30 hardware board', uk: 'розмір рушія — вистачає місця на платі за $30' },
};

for (const [key, { en, uk }] of Object.entries(BLOCKS)) {
  const row = db.prepare('SELECT is_html, section FROM content_blocks WHERE key = ? LIMIT 1').get(key);
  if (!row) { console.warn('SKIP (unknown key):', key); continue; }
  setValue(key, 'en', en, row.is_html, row.section);
  setValue(key, 'uk', uk, row.is_html, row.section);
}
console.log(`Updated ${Object.keys(BLOCKS).length} content_blocks keys (EN+UK).`);

// ---- FAQ rows: rewritten as the questions a musician actually asks ----
const FAQS = [
  {
    question_en: 'Will pushing this synth choke my computer?',
    answer_en: 'No. Turning up Brightness or stacking harmonics costs the <strong>same CPU</strong> whether the tone is a plain sine or a screaming wall of overtones — the whole harmonic stack is computed in one shot instead of one oscillator per harmonic. You’ll never hit a CPU wall just from cranking one knob.',
    question_uk: 'Чи не задушить мій комп’ютер цей синт, якщо його викрутити на повну?',
    answer_uk: 'Ні. Збільшення Яскравості чи гармонік коштує <strong>однаково для CPU</strong>, чи тон — простий синус, чи дзвінка стіна обертонів: уся гармонічна стопка рахується за один прохід, а не осцилятор на кожну гармоніку. Ти ніколи не впрешся в стелю CPU просто крутячи одну ручку.',
  },
  {
    question_en: 'Does it sound harsh or “digital” if I push it hard?',
    answer_en: 'The clean oscillator never can — it’s mathematically incapable of that kind of digital nastiness on its own. Push Drive, Fold, FM or feedback hard enough and yes, those <em>can</em> turn gritty and harsh — that’s intentional character, the same kind PPG and DX7 fans love. The built-in meter tells you the moment it’s happening, and HQ mode cleans it up whenever you don’t want it.',
    question_uk: 'Чи звучить різко/«цифрово», якщо викрутити на повну?',
    answer_uk: 'Чистий осцилятор — ніколи: він математично не здатен на таку цифрову бридню сам по собі. А ось Drive, Fold, FM чи зворотний зв’язок, викручені достатньо, <em>можуть</em> стати брудними й різкими — це навмисний характер, той самий, за який люблять PPG і DX7. Вбудований індикатор скаже, коли це відбувається, а HQ-режим прибере це, коли не хочеш.',
  },
  {
    question_en: 'Why doesn’t HQ mode lower my CPU usage?',
    answer_en: 'Because the oscillator was never the expensive part — HQ’s only job is to catch harshness coming from the dirt and FM stages before it reaches your ears, not to make the synth “more efficient.” Turn it on when a patch needs cleaning up, leave it off otherwise; it costs roughly the same CPU either way, plus a small fixed bit of latency while it’s on.',
    question_uk: 'Чому HQ-режим не знижує навантаження на CPU?',
    answer_uk: 'Бо дорогою частиною ніколи не був осцилятор — єдина робота HQ — ловити різкість від брудних стадій і FM, поки вона не дійшла до вух, а не робити синт «ефективнішим». Вмикай, коли патч треба почистити, вимикай в інших випадках; CPU коштує приблизно однаково в обох випадках, плюс невелика фіксована затримка, поки він увімкнений.',
  },
  {
    question_en: 'If I freeze or bounce a track, will it still sound right months later?',
    answer_en: 'Yes, always — on this laptop, a different one, a Mac, a PC, even a different buffer size. Cosine renders the <strong>exact same audio, every single time</strong>, so a freeze from six months ago still sounds identical today. No re-rendering and hoping for the best.',
    question_uk: 'Якщо заморозити чи звести трек, чи звучатиме він так само через кілька місяців?',
    answer_uk: 'Так, завжди — на цьому ноутбуку, на іншому, на Mac, на PC, навіть при іншому розмірі буфера. Cosine рендерить <strong>абсолютно той самий звук щоразу</strong>, тож фриз піврічної давності звучить ідентично й сьогодні. Жодного «перерендерити й сподіватись».',
  },
  {
    question_en: 'VST3 and CLAP — where’s AU / AAX?',
    answer_en: 'VST3 + CLAP at launch. AU (for Logic users) is planned. AAX (Pro Tools) isn’t — it would require a hardware dongle, and we’re not doing that to you.',
    question_uk: 'VST3 і CLAP — а де AU / AAX?',
    answer_uk: 'VST3 + CLAP на старті. AU (для користувачів Logic) заплановано. AAX (Pro Tools) — ні: він вимагав би апаратного донгла, а ми цього тобі не зробимо.',
  },
  {
    question_en: 'Where does the name come from?',
    answer_en: 'Every note is literally built by adding up cosine waves, one per harmonic — that’s the entire trick, no secret sauce hiding underneath. The name just says what it does. The domain’s still being sorted before launch; the plugin itself is finished and already tested.',
    question_uk: 'Звідки назва?',
    answer_uk: 'Кожна нота буквально будується додаванням косинусних хвиль, по одній на гармоніку, — це весь фокус, ніякого прихованого соусу під цим немає. Назва просто каже, що воно робить. Домен ще узгоджується перед релізом; сам плагін уже готовий і перевірений.',
  },
];

const rows = db.prepare('SELECT id, sort_order FROM faqs ORDER BY sort_order').all();
const updateFaq = db.prepare(`
  UPDATE faqs SET question_en = ?, answer_en = ?, question_uk = ?, answer_uk = ?, updated_at = datetime('now')
  WHERE id = ?
`);
rows.forEach((row, i) => {
  const f = FAQS[i];
  if (!f) return;
  updateFaq.run(f.question_en, f.answer_en, f.question_uk, f.answer_uk, row.id);
});
console.log(`Updated ${Math.min(rows.length, FAQS.length)} FAQ rows.`);
