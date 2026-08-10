/* ============================================================
   mlc sim — type registry + tab/content specs.

   Mirrors what the Rust primitives DECLARE, in the same shape the
   modal derives its tabs from: the first tab is the type's own
   subject (named after it), "Подписи" appears only where the type
   labels someone else's points, "Размещение" is always last.
   Approved concepts: crates/mlc-ui-lab/designs/2026-08-10-<theme>-modal/ (v2 files)

   This file carries no styling and no geometry — those come from
   the generated tokens. It carries CONTENT: which groups exist and
   which rows live in them.
   ============================================================ */

(function () {
  'use strict';

  // ---- strings ---------------------------------------------------------
  // A label the app already has goes through the app's own table (`i18n.js`,
  // generated from `mlc-core/src/i18n`), so the sim never spells a word the
  // app has already spelled. A label the app does NOT have yet is concept
  // vocabulary — it stays authored here and becomes a real key when the
  // archetype reaches `mlc-core`. `fb` is what the concept says; passing it
  // is deliberate, not a fallback for a typo.
  const I18N = window.SIM_I18N || {};
  let lang = 'ru';
  function t(table, key, fb) {
    const row = (I18N[table] || {})[key];
    return (row && (row[lang] || row.en)) || fb || key;
  }

  // ---- shared groups ---------------------------------------------------

  // Timeframe visibility: six numeric classes with range sliders, the control
  // the app actually ships (primitive_settings_placement.rs), not chips.
  const TF = {
    title: 'Виден на таймфреймах',
    rows: [
      { k: 'chk', lab: 'Диапазоны', on: true },
      { k: 'range', lab: 'Секунды', from: 0, to: 100, cap: '1с–30с' },
      { k: 'range', lab: 'Минуты', from: 0, to: 100, cap: '1м–45м' },
      { k: 'range', lab: 'Часы', from: 0, to: 100, cap: '1Ч–12Ч' },
      { k: 'range', lab: 'Дни', from: 0, to: 100, cap: '1Д–3Д' },
      { k: 'range', lab: 'Недели', from: 0, to: 100, cap: '1Н' },
      { k: 'range', lab: 'Месяцы', from: 0, to: 100, cap: '1М' },
    ],
  };

  const BEHAVIOUR = {
    title: 'Поведение',
    rows: [
      { k: 'chk', lab: t('MENU_KEY', 'Lock', 'Заблокировать') },
      { k: 'chk', lab: 'Скрыть' },
      { k: 'chk', lab: 'На всех графиках символа' },
    ],
  };

  const points = (names, note) => ({
    title: 'Точки',
    hint: names.length + '',
    rows: [{ k: 'table', head: ['Точка', 'Цена', 'Время'], rows: names.map((n, i) => [n, fmtPrice(65100 + i * 180), fmtTime(i)]) }]
      .concat(note ? [{ k: 'note', text: note }] : []),
  });

  const placement = (names, note) => ({ id: 'placement', label: 'Размещение', groups: [TF, points(names, note), BEHAVIOUR] });

  function fmtPrice(v) { return v.toLocaleString('ru-RU', { minimumFractionDigits: 2, maximumFractionDigits: 2 }); }
  function fmtTime(i) { return `1${i % 2 ? 0 : 1}.08 0${(i % 8) + 1}:00`; }

  const labelRows = (what) => [
    { k: 'chk', lab: t('SETTINGS_KEY', 'Show', 'Показывать'), on: true },
    { k: 'chk', lab: 'Наследовать цвет линии', on: true },
    { k: 'color', lab: 'Цвет подписи', v: 'var(--mlc-colors-accent)', dim: true },
    { k: 'num', lab: 'Размер', v: 12 },
    { k: 'chk', lab: 'Жирный' },
    { k: 'note', text: what },
  ];

  const PLATE = {
    title: 'Плашка',
    rows: [
      { k: 'chk', lab: 'Плашка под подписью', on: true },
      { k: 'color', lab: 'Цвет плашки', v: 'var(--mlc-modal-background)' },
      { k: 'num', lab: 'Скругление', v: 3 },
      { k: 'num', lab: 'Отступы', v: 4 },
    ],
  };

  const LINE = (title) => ({
    title: title || t('TOOLBAR_MENU_KEY', 'Line', 'Линия'),
    rows: [
      { k: 'color', lab: t('SETTINGS_KEY', 'Color', 'Цвет'), v: 'var(--mlc-colors-accent)' },
      { k: 'slider', lab: 'Толщина', pct: 24, v: '1.0' },
      { k: 'dd', lab: t('CONFIG_KEY', 'Style', 'Стиль'), v: t('STYLE_KEY', 'Solid', 'Сплошная') },
    ],
  });

  const FILL = (title, on) => ({
    title: title || t('CONFIG_KEY', 'Fill', 'Заливка'),
    rows: [
      { k: 'chk', lab: 'Заливать', on: !!on },
      { k: 'color', lab: t('SETTINGS_KEY', 'Color', 'Цвет'), v: 'var(--mlc-colors-accent)' },
      { k: 'slider', lab: 'Прозрачность', pct: 10, v: '10%' },
    ],
  });

  const EXTEND = {
    title: t('CONFIG_KEY', 'Extend', 'Продление'),
    rows: [{ k: 'chk', lab: 'Влево' }, { k: 'chk', lab: 'Вправо' }],
  };

  // ---- archetypes ------------------------------------------------------

  const A = {};

  // Ladder — fib / gann / pitchfork. `unit` decides how a value reads.
  A.ladder = (opts) => ({
    tabs: [
      {
        id: 'levels',
        label: opts.subject,
        groups: [
          {
            title: 'Общее',
            rows: [
              { k: 'mode', lab: '', opts: ['Моно', 'Палитра'], on: 1 },
              { k: 'slider', lab: 'Толщина', pct: 24, v: '1.0' },
              { k: 'dd', lab: 'Стиль', v: 'Сплошная' },
              { k: 'chk3', labs: ['Продлить влево', 'Продлить вправо', 'Трендовая линия'], on: [false, false, true] },
            ],
          },
          { title: 'Палитры', rows: [{ k: 'ramps' }] },
          {
            title: 'Заливка',
            rows: [{ k: 'fillmaster', lab: 'Заливка до следующего уровня', pct: 10, v: '10%', on: true }],
          },
          { title: opts.subject, hint: opts.values.length + '', rows: [{ k: 'ladder', values: opts.values, unit: opts.unit }] },
        ],
      },
      { id: 'labels', label: 'Подписи', groups: [{ title: 'Подписи уровней', rows: labelRows('Подпись берёт цвет своего уровня, пока включено наследование.') }, PLATE] },
      placement(['1', '2']),
    ],
  });

  A.wave = {
    tabs: [
      {
        id: 'waves',
        label: 'Волны',
        groups: [
          LINE(),
          { title: 'Степень', rows: [{ k: 'dd', lab: 'Степень волны', v: 'Minor' }, { k: 'note', text: 'Subminuette · Minuette · Minute · Minor · Intermediate · Primary · Cycle · Supercycle' }] },
          { title: 'Нотация', rows: [{ k: 'pills', opts: ['1 2 3 4 5', 'I II III IV V', '① ② ③ ④ ⑤'], on: 0 }, { k: 'note', text: 'Для коррекции — A B C, для комбинаций — W X Y (Z).' }] },
          { title: 'Тип коррекции', hint: 'только у коррекции', rows: [{ k: 'dd', lab: 'Тип', v: 'Зигзаг' }] },
          FILL('Заливка между волнами', false),
        ],
      },
      {
        id: 'labels',
        label: 'Подписи',
        groups: [
          { title: 'Подписи волн', rows: labelRows('Имена берутся из степени волны.').concat([{ k: 'chk', lab: 'Показывать степень' }]) },
          PLATE,
          { title: 'Свои подписи', hint: 'переопределение', rows: [{ k: 'table', head: ['Волна', 'Подпись'], rows: [['0', '0'], ['1', '1'], ['2', '2'], ['3', '3'], ['4', '4'], ['5', '5']] }] },
        ],
      },
      placement(['0', '1', '2', '3', '4', '5']),
    ],
  };

  A.pattern = {
    tabs: [
      {
        id: 'ratios',
        label: 'Пропорции',
        groups: [
          { title: 'Подтип', rows: [{ k: 'dd', lab: 'Паттерн', v: 'Gartley' }, { k: 'note', text: 'Gartley · Bat · Butterfly · Crab · Shark · Свой' }] },
          LINE('Линии'),
          FILL('Тело паттерна', true),
          {
            title: 'Целевые отношения',
            rows: [{
              k: 'table', head: ['Отрезок', 'Цель', 'Факт', 'Допуск'], rows: [
                ['XA→AB', '0.618', { t: '0.621', cls: 'good' }, '3%'],
                ['AB→BC', '0.382', { t: '0.394', cls: 'good' }, '5%'],
                ['BC→CD', '1.272', { t: '1.512', cls: 'bad' }, '5%'],
                ['XA→AD', '0.786', { t: '0.780', cls: 'good' }, '3%'],
              ],
            }],
          },
          { title: 'Оценка', rows: [{ k: 'color', lab: 'Цвет попадания', v: 'var(--mlc-colors-success)' }, { k: 'color', lab: 'Цвет промаха', v: 'var(--mlc-colors-danger)' }, { k: 'chk', lab: 'Красить линии по попаданию' }] },
        ],
      },
      {
        id: 'labels',
        label: 'Подписи',
        groups: [
          { title: 'Подписи точек', rows: labelRows('Имена точек — X A B C D.') },
          { title: 'Значения на графике', rows: [{ k: 'chk', lab: 'Показывать отношения', on: true }, { k: 'chk', lab: 'Показывать длину отрезков' }, { k: 'num', lab: 'Знаков после запятой', v: 3 }, { k: 'dd', lab: 'Сторона от отрезка', v: 'Снаружи' }] },
          PLATE,
        ],
      },
      placement(['X', 'A', 'B', 'C', 'D']),
    ],
  };

  A.position = {
    tabs: [
      {
        id: 'trade',
        label: 'Сделка',
        groups: [
          { title: 'Объём', rows: [{ k: 'num', lab: 'Количество', v: '1.00' }, { k: 'num', lab: 'Размер счёта', v: '10000' }, { k: 'num', lab: 'Риск на сделку, %', v: '1.0' }, { k: 'chk', lab: 'Считать объём от риска' }] },
          { title: 'Зоны', rows: [{ k: 'color', lab: 'Зона стопа', v: 'var(--mlc-colors-danger)' }, { k: 'color', lab: 'Зона цели', v: 'var(--mlc-colors-success)' }, { k: 'slider', lab: 'Прозрачность', pct: 18, v: '18%' }] },
          { title: 'Линии', rows: [{ k: 'color', lab: 'Вход', v: 'var(--mlc-colors-accent)' }, { k: 'color', lab: 'Стоп', v: 'var(--mlc-colors-danger)' }, { k: 'color', lab: 'Цель', v: 'var(--mlc-colors-success)' }, { k: 'dd', lab: 'Стиль', v: 'Сплошная' }] },
          { title: 'Отображение', rows: [{ k: 'pills', opts: ['Валюта', 'Тики', 'Проценты'], on: 0 }, { k: 'chk', lab: 'Показывать P&L', on: true }, { k: 'chk', lab: 'Показывать объём', on: true }] },
          { title: 'Результат', hint: 'считается', rows: [{ k: 'stat', lab: 'Риск', v: '−437.59', cls: 'bad' }, { k: 'stat', lab: 'Цель', v: '+875.19', cls: 'good' }, { k: 'hr' }, { k: 'stat', lab: 'Отношение', v: '1 : 2.00' }, { k: 'note', text: 'Не редактируется — считается от точек входа, стопа и цели.' }] },
        ],
      },
      {
        id: 'labels',
        label: 'Подписи',
        groups: [
          { title: 'Подписи', rows: labelRows('Подписи входа, стопа и цели.') },
          { title: 'Что писать', rows: [{ k: 'chk', lab: 'Имя точки', on: true }, { k: 'chk', lab: 'Цену', on: true }, { k: 'chk', lab: 'Расстояние в %', on: true }, { k: 'chk', lab: 'Сумму риска и цели' }, { k: 'num', lab: 'Знаков после запятой', v: 2 }] },
          PLATE,
        ],
      },
      placement(['Вход', 'Стоп', 'Цель']),
    ],
  };

  A.measure = {
    tabs: [
      {
        id: 'show',
        label: 'Что показывать',
        groups: [
          { title: 'Величины', rows: [{ k: 'chk', lab: 'Изменение цены', on: true }, { k: 'chk', lab: 'Проценты', on: true }, { k: 'chk', lab: 'Число баров', on: true }, { k: 'chk', lab: 'Время' }, { k: 'chk', lab: 'Объём за диапазон' }, { k: 'chk', lab: 'Средняя цена' }, { k: 'chk', lab: 'Угол наклона' }] },
          { title: 'Формат', rows: [{ k: 'num', lab: 'Знаков после запятой', v: 2 }, { k: 'chk', lab: 'Разделитель разрядов', on: true }, { k: 'dd', lab: 'Единица объёма', v: 'Авто (K / M)' }] },
          { title: 'Линия и стрелка', rows: [{ k: 'color', lab: 'Цвет', v: 'var(--mlc-colors-accent)' }, { k: 'slider', lab: 'Толщина', pct: 24, v: '1.0' }, { k: 'chk', lab: 'Стрелка на конце', on: true }] },
          { title: 'Область', rows: [{ k: 'chk', lab: 'Заливать', on: true }, { k: 'slider', lab: 'Прозрачность', pct: 15, v: '15%' }, { k: 'chk', lab: 'Красить по знаку', on: true }, { k: 'color', lab: 'Рост', v: 'var(--mlc-colors-success)' }, { k: 'color', lab: 'Падение', v: 'var(--mlc-colors-danger)' }] },
        ],
      },
      {
        id: 'labels',
        label: 'Подписи',
        groups: [
          { title: 'Текст', rows: [{ k: 'num', lab: 'Размер', v: 12 }, { k: 'color', lab: 'Цвет', v: 'var(--mlc-colors-text-primary)' }, { k: 'chk', lab: 'Наследовать цвет линии' }] },
          { title: 'Плашка', rows: [{ k: 'pills', opts: ['Сверху', 'В центре', 'Снизу'], on: 1 }, { k: 'chk', lab: 'Фон плашки', on: true }, { k: 'color', lab: 'Цвет фона', v: 'var(--mlc-modal-background)' }] },
        ],
      },
      placement(['Начало', 'Конец']),
    ],
  };

  A.text = {
    tabs: [
      {
        id: 'text',
        label: 'Текст',
        groups: [
          { title: 'Содержимое', rows: [{ k: 'area', v: 'Пробой уровня, ждём ретест' }, { k: 'note', text: 'Подстановки: {цена} {время} {символ} {таймфрейм}' }] },
          { title: 'Шрифт', rows: [{ k: 'dd', lab: 'Гарнитура', v: 'Inter' }, { k: 'num', lab: 'Размер', v: 13 }, { k: 'pills', lab: 'Начертание', opts: ['Ж', 'К', 'Ч'], on: 0 }, { k: 'color', lab: 'Цвет', v: 'var(--mlc-colors-text-primary)' }] },
          { title: 'Раскладка', rows: [{ k: 'pills', opts: ['По левому', 'По центру', 'По правому'], on: 0 }, { k: 'chk', lab: 'Перенос по словам', on: true }, { k: 'chk', lab: 'Ширина по содержимому', on: true }, { k: 'num', lab: 'Максимальная ширина', v: 240 }] },
          { title: 'Плашка', rows: [{ k: 'chk', lab: 'Рамка', on: true }, { k: 'color', lab: 'Цвет рамки', v: 'var(--mlc-colors-accent)' }, { k: 'chk', lab: 'Фон', on: true }, { k: 'color', lab: 'Цвет фона', v: 'var(--mlc-modal-background)' }, { k: 'num', lab: 'Скругление', v: 4 }, { k: 'num', lab: 'Отступы', v: 6 }, { k: 'chk', lab: 'Тень' }] },
          { title: 'Указатель', hint: 'выноска, ценовая заметка', rows: [{ k: 'chk', lab: 'Хвостик к точке', on: true }, { k: 'dd', lab: 'Наконечник', v: 'Стрелка' }] },
        ],
      },
      { id: 'placement', label: 'Размещение', groups: [{ title: 'Якорь', rows: [{ k: 'pills', opts: ['К цене и бару', 'К бару', 'К экрану'], on: 0 }, { k: 'pills', opts: ['Сверху', 'Справа', 'Снизу', 'Слева'], on: 1 }] }, TF, points(['Якорь', 'Плашка']), BEHAVIOUR] },
    ],
  };

  A.table = {
    tabs: [
      {
        id: 'cells',
        label: 'Ячейки',
        groups: [
          { title: 'Размер', rows: [{ k: 'stepper', lab: 'Строк', v: 4 }, { k: 'stepper', lab: 'Колонок', v: 3 }] },
          { title: 'Содержимое', hint: '4 × 3', rows: [{ k: 'cells', cols: 3, rows: [['Уровень', 'Цена', 'Комментарий'], ['Сопротивление', '65 400', 'дневной максимум'], ['Вход', '65 100', 'по ретесту'], ['Стоп', '64 660', 'под лоем']] }] },
          { title: 'Колонки', rows: [{ k: 'table', head: ['Колонка', 'Выравнивание', 'Ширина'], rows: [['1', 'По левому', 'авто'], ['2', 'По правому', '70'], ['3', 'По левому', 'авто']] }] },
          { title: 'Сетка и шапка', rows: [{ k: 'chk', lab: 'Показывать линии', on: true }, { k: 'color', lab: 'Цвет линий', v: 'var(--mlc-colors-border-light)' }, { k: 'chk', lab: 'Первая строка — шапка', on: true }, { k: 'chk', lab: 'Чередовать строки' }] },
          { title: 'Текст и фон', rows: [{ k: 'num', lab: 'Размер', v: 12 }, { k: 'color', lab: 'Цвет текста', v: 'var(--mlc-colors-text-primary)' }, { k: 'color', lab: 'Фон таблицы', v: 'var(--mlc-colors-toolbar-bg)' }, { k: 'slider', lab: 'Прозрачность', pct: 90, v: '90%' }] },
        ],
      },
      { id: 'placement', label: 'Размещение', groups: [{ title: 'Якорь', rows: [{ k: 'pills', opts: ['К цене и бару', 'К экрану'], on: 1 }, { k: 'pills', opts: ['Слева сверху', 'Справа сверху', 'Слева снизу', 'Справа снизу'], on: 1 }] }, TF, BEHAVIOUR] },
    ],
  };

  A.shape = (subject, extra) => ({
    tabs: [
      { id: 'shape', label: subject, groups: [LINE('Контур')].concat(extra || []).concat([FILL('Заливка', true), EXTEND, { title: 'Текст на фигуре', rows: [{ k: 'chk', lab: 'Показывать' }, { k: 'num', lab: 'Размер', v: 12 }, { k: 'dd', lab: 'Положение', v: 'По центру' }] }]) },
      placement(['1', '2'], 'У полилинии список растёт по числу точек — та же таблица со своей прокруткой.'),
    ],
  });

  A.regression = {
    tabs: [
      {
        id: 'sigma',
        label: 'Отклонения',
        groups: [
          { title: 'Расчёт', rows: [{ k: 'dd', lab: 'Источник', v: 'Close' }, { k: 'dd', lab: 'Период', v: 'По точкам' }, { k: 'num', lab: 'Число баров', v: 120 }] },
          { title: 'Сигмы', rows: [{ k: 'table', head: ['', 'Множитель', 'Цвет', 'Заливка'], rows: [['±1σ', '1.0', { sw: 'var(--mlc-colors-accent)' }, { chk: true }], ['±2σ', '2.0', { sw: 'var(--mlc-colors-warning)' }, { chk: true }], ['±3σ', '3.0', { sw: 'var(--mlc-colors-danger)' }, { chk: false }]] }, { k: 'slider', lab: 'Прозрачность заливки', pct: 10, v: '10%' }] },
          { title: 'Базовая линия', rows: [{ k: 'chk', lab: 'Показывать', on: true }, { k: 'color', lab: 'Цвет', v: 'var(--mlc-colors-accent)' }, { k: 'slider', lab: 'Толщина', pct: 30, v: '1.5' }] },
          EXTEND,
          { title: 'Показатели', hint: 'считаются', rows: [{ k: 'chk', lab: 'Показывать наклон' }, { k: 'chk', lab: 'Показывать R²', on: true }, { k: 'stat', lab: 'Наклон', v: '+18.4 / бар' }, { k: 'stat', lab: 'R²', v: '0.87' }] },
        ],
      },
      placement(['Начало', 'Конец']),
    ],
  };

  A.brush = {
    tabs: [
      {
        id: 'stroke',
        label: 'Мазок',
        groups: [
          { title: 'Мазок', rows: [{ k: 'color', lab: 'Цвет', v: 'var(--mlc-colors-accent)' }, { k: 'slider', lab: 'Размер', pct: 35, v: '3.0' }, { k: 'slider', lab: 'Прозрачность', pct: 100, v: '100%' }, { k: 'dd', lab: 'Стиль линии', v: 'Сплошная' }] },
          { title: 'Форма', rows: [{ k: 'chk', lab: 'Сглаживание', on: true }, { k: 'slider', lab: 'Сила сглаживания', pct: 50, v: '50%' }, { k: 'chk', lab: 'Скруглять концы', on: true }, { k: 'chk', lab: 'Замыкать контур' }] },
          FILL('Заливка у замкнутого контура', false),
        ],
      },
      placement(['Начало', 'Конец'], 'У кисти точки не правятся поштучно — показываются рамка мазка и число точек.'),
    ],
  };

  A.icon = {
    tabs: [
      {
        id: 'icon',
        label: 'Значок',
        groups: [
          { title: 'Символ', rows: [{ k: 'icons', opts: ['▲', '▼', '⚑', '★', '●', '◆', '✕', '✓', '❗', '❓', '💡', '🔒', '📌', '🔔', '👁', '💬'], on: 0 }] },
          { title: 'Отрисовка', rows: [{ k: 'color', lab: 'Цвет', v: 'var(--mlc-colors-success)' }, { k: 'slider', lab: 'Размер', pct: 40, v: '18' }, { k: 'chk', lab: 'Обводка' }] },
          { title: 'Подпись рядом', rows: [{ k: 'chk', lab: 'Показывать', on: true }, { k: 'dd', lab: 'Текст', v: 'Вход по тренду' }, { k: 'num', lab: 'Размер', v: 12 }, { k: 'dd', lab: 'Сторона', v: 'Справа' }] },
        ],
      },
      placement(['Точка']),
    ],
  };

  // ---- types -----------------------------------------------------------

  const FIB = [0, 0.236, 0.382, 0.5, 0.618, 0.786, 1];
  const GANN = [8, 4, 3, 2, 1, 0.5, 0.333];

  const TYPES = [
    { id: 'fib_retracement', name: 'Fib Retracement', spec: A.ladder({ subject: 'Уровни', values: FIB, unit: 'ratio' }) },
    { id: 'gann_fan', name: 'Gann Fan', spec: A.ladder({ subject: 'Углы', values: GANN, unit: 'gann' }) },
    { id: 'pitchfork', name: 'Pitchfork', spec: A.ladder({ subject: 'Уровни', values: [0.25, 0.5, 0.75, 1], unit: 'ratio' }) },
    { id: 'elliott_impulse', name: 'Elliott Impulse', spec: A.wave },
    { id: 'xabcd_pattern', name: 'XABCD Pattern', spec: A.pattern },
    { id: 'long_position', name: 'Long Position', spec: A.position },
    { id: 'price_date_range', name: 'Price & Date Range', spec: A.measure },
    { id: 'callout', name: 'Callout', spec: A.text },
    { id: 'table', name: 'Table', spec: A.table },
    { id: 'rectangle', name: 'Rectangle', spec: A.shape('Фигура') },
    {
      id: 'parallel_channel', name: 'Parallel Channel', spec: A.shape('Канал', [{
        title: 'Средняя линия',
        rows: [{ k: 'chk', lab: 'Показывать', on: true }, { k: 'color', lab: 'Цвет', v: 'var(--mlc-colors-text-muted)' }, { k: 'dd', lab: 'Стиль', v: 'Пунктир' }],
      }]),
    },
    { id: 'regression_trend', name: 'Regression Trend', spec: A.regression },
    { id: 'brush', name: 'Brush', spec: A.brush },
    { id: 'flag', name: 'Flag', spec: A.icon },
  ];

  // System templates a type offers, mirroring PrimitiveTemplate::system_templates:
  // a ladder gets the four named sets, everything else a single "Standard".
  function templatesFor(type) {
    const ladder = type.spec.tabs[0].groups.some((g) => g.rows.some((r) => r.k === 'ladder'));
    return ladder
      ? ['Standard', 'Extended', 'Key levels only', 'All levels']
      : ['Standard'];
  }

  // ---- application modals ----------------------------------------------
  // Not object-bound: these hang off the toolbars. Same shell, same tokens —
  // what differs is that their tab strip is authored, not derived from a type.

  const APP_MODALS = [
    {
      id: 'chart_settings', name: 'Настройки графика', icon: 'settings', rail: true, w: 620, h: 560,
      tabs: [
        {
          id: 'instrument', label: 'Инструмент', icon: 'candlestick', groups: [
            { title: 'Свечи', rows: [{ k: 'color', lab: 'Тело роста', v: 'var(--mlc-colors-success)' }, { k: 'color', lab: 'Тело падения', v: 'var(--mlc-colors-danger)' }, { k: 'chk', lab: 'Полые свечи' }, { k: 'chk', lab: 'Фитили', on: true }, { k: 'color', lab: 'Цвет фитиля', v: 'var(--mlc-colors-text-muted)' }] },
            { title: 'Границы', rows: [{ k: 'chk', lab: 'Показывать', on: true }, { k: 'slider', lab: 'Толщина', pct: 20, v: '1.0' }] },
            { title: 'Прочее', rows: [{ k: 'chk', lab: 'Последняя цена', on: true }, { k: 'chk', lab: 'Предыдущее закрытие' }, { k: 'chk', lab: 'Высокая и низкая' }] },
          ],
        },
        {
          id: 'status', label: 'Статус-строка', icon: 'legend', groups: [
            { title: 'Показывать', rows: [{ k: 'chk', lab: 'Тикер', on: true }, { k: 'chk', lab: 'Значения OHLC', on: true }, { k: 'chk', lab: 'Изменение', on: true }, { k: 'chk', lab: 'Объём', on: true }, { k: 'chk', lab: 'Индикаторы', on: true }] },
            { title: 'Оформление', rows: [{ k: 'num', lab: 'Размер', v: 12 }, { k: 'dd', lab: 'Положение', v: 'Слева сверху' }, { k: 'slider', lab: 'Прозрачность фона', pct: 60, v: '60%' }] },
          ],
        },
        {
          id: 'scales', label: 'Шкалы и линии', icon: 'grid', groups: [
            { title: 'Сетка', rows: [{ k: 'chk', lab: 'Вертикальные', on: true }, { k: 'chk', lab: 'Горизонтальные', on: true }, { k: 'color', lab: 'Цвет', v: 'var(--mlc-colors-border-light)' }, { k: 'dd', lab: 'Стиль', v: 'Сплошная' }] },
            { title: 'Ценовая шкала', rows: [{ k: 'dd', lab: 'Режим', v: 'Авто' }, { k: 'num', lab: 'Ширина', v: 70 }, { k: 'chk', lab: 'Проценты' }, { k: 'chk', lab: 'Обратная' }] },
            { title: 'Временная шкала', rows: [{ k: 'chk', lab: 'Показывать секунды' }, { k: 'chk', lab: 'Показывать дату', on: true }, { k: 'num', lab: 'Высота', v: 30 }] },
            { title: 'Перекрестье', rows: [{ k: 'color', lab: 'Цвет', v: 'var(--mlc-colors-text-muted)' }, { k: 'dd', lab: 'Стиль', v: 'Пунктир' }] },
          ],
        },
        {
          id: 'look', label: 'Оформление', icon: 'palette', groups: [
            { title: 'Фон', rows: [{ k: 'pills', opts: ['Сплошной', 'Градиент'], on: 0 }, { k: 'color', lab: 'Цвет', v: 'var(--mlc-colors-toolbar-bg)' }] },
            { title: 'Водяной знак', rows: [{ k: 'chk', lab: 'Показывать', on: true }, { k: 'slider', lab: 'Прозрачность', pct: 20, v: '20%' }, { k: 'num', lab: 'Размер', v: 52 }] },
            { title: 'Поля', rows: [{ k: 'slider', lab: 'Сверху', pct: 10, v: '10%' }, { k: 'slider', lab: 'Снизу', pct: 8, v: '8%' }, { k: 'num', lab: 'Отступ справа, баров', v: 10 }] },
          ],
        },
      ],
    },
    {
      id: 'indicator_settings', name: 'Настройки индикатора', icon: 'indicators', w: 520, h: 470,
      tabs: [
        { id: 'inputs', label: 'Параметры', groups: [{ title: 'Вход', rows: [{ k: 'dd', lab: 'Источник', v: 'Close' }, { k: 'num', lab: 'Период', v: 14 }, { k: 'dd', lab: 'Сглаживание', v: 'EMA' }, { k: 'num', lab: 'Период сглаживания', v: 9 }] }, { title: 'Уровни', rows: [{ k: 'chk', lab: 'Верхний', on: true }, { k: 'num', lab: 'Значение', v: 70 }, { k: 'chk', lab: 'Нижний', on: true }, { k: 'num', lab: 'Значение', v: 30 }] }] },
        { id: 'style', label: 'Стиль', groups: [{ title: 'Линии', rows: [{ k: 'color', lab: 'Основная', v: 'var(--mlc-colors-accent)' }, { k: 'slider', lab: 'Толщина', pct: 24, v: '1.0' }, { k: 'dd', lab: 'Стиль', v: 'Сплошная' }] }, { title: 'Заливка', rows: [{ k: 'chk', lab: 'Между уровнями', on: true }, { k: 'slider', lab: 'Прозрачность', pct: 12, v: '12%' }] }, { title: 'Точность', rows: [{ k: 'dd', lab: 'Знаков', v: 'По умолчанию' }] }] },
        { id: 'signals', label: 'Сигналы', groups: [{ title: 'Условия', rows: [{ k: 'chk', lab: 'Пересечение вверх', on: true }, { k: 'chk', lab: 'Пересечение вниз', on: true }, { k: 'chk', lab: 'Выход из зоны' }] }, { title: 'Отметки', rows: [{ k: 'dd', lab: 'Форма', v: 'Треугольник' }, { k: 'num', lab: 'Размер', v: 10 }] }] },
        { id: 'visibility', label: 'Видимость', groups: [{ title: 'Виден на таймфреймах', rows: [{ k: 'chk', lab: 'Диапазоны', on: true }, { k: 'range', lab: 'Минуты', from: 0, to: 100, cap: '1м–45м' }, { k: 'range', lab: 'Часы', from: 0, to: 100, cap: '1Ч–12Ч' }, { k: 'range', lab: 'Дни', from: 0, to: 100, cap: '1Д–3Д' }] }] },
      ],
    },
    {
      id: 'watchlist', name: 'Список наблюдения', icon: 'watchlist', w: 520, h: 470,
      tabs: [
        { id: 'overview', label: 'Обзор', groups: [{ title: 'Инструменты', rows: [{ k: 'table', head: ['Символ', 'Цена', 'Изм.'], rows: [['BTCUSDT', '65 100.00', { t: '+1.24%', cls: 'good' }], ['ETHUSDT', '3 420.55', { t: '−0.42%', cls: 'bad' }], ['SOLUSDT', '182.30', { t: '+3.10%', cls: 'good' }]] }] }] },
        { id: 'groups', label: 'Группы', groups: [{ title: 'Группы', rows: [{ k: 'pills', opts: ['Крипта', 'Форекс', 'Акции США'], on: 0 }, { k: 'chk', lab: 'Синхронизировать с графиком', on: true }] }] },
        { id: 'settings', label: 'Настройки', groups: [{ title: 'Колонки', rows: [{ k: 'chk', lab: 'Цена', on: true }, { k: 'chk', lab: 'Изменение', on: true }, { k: 'chk', lab: 'Объём' }, { k: 'chk', lab: 'Спред' }] }] },
      ],
    },
    {
      id: 'alerts', name: 'Алерты', icon: 'alert', w: 520, h: 470,
      tabs: [
        { id: 'settings', label: 'Настройки', groups: [{ title: 'Условие', rows: [{ k: 'dd', lab: 'Тип', v: 'Пересечение цены' }, { k: 'num', lab: 'Значение', v: '65 400' }, { k: 'dd', lab: 'Направление', v: 'Вверх' }] }, { title: 'Срок', rows: [{ k: 'chk', lab: 'Однократный', on: true }, { k: 'dd', lab: 'Истекает', v: 'Через сутки' }] }] },
        { id: 'notify', label: 'Уведомления', groups: [{ title: 'Каналы', rows: [{ k: 'chk', lab: 'Звук', on: true }, { k: 'chk', lab: 'Всплывающее окно', on: true }, { k: 'chk', lab: 'Телеграм' }, { k: 'chk', lab: 'Вебхук' }] }, { title: 'Сообщение', rows: [{ k: 'area', v: '{символ} пробил {значение} на {таймфрейм}' }] }] },
        { id: 'list', label: 'Список', groups: [{ title: 'Активные', rows: [{ k: 'table', head: ['Символ', 'Условие', 'Статус'], rows: [['BTCUSDT', '> 65 400', { t: 'ждёт', cls: '' }], ['ETHUSDT', '< 3 300', { t: 'сработал', cls: 'good' }]] }] }] },
      ],
    },
    {
      id: 'user_settings', name: 'Настройки приложения', icon: 'user', w: 520, h: 440,
      tabs: [
        { id: 'general', label: 'Общие', groups: [{ title: 'Интерфейс', rows: [{ k: 'dd', lab: 'Язык', v: 'Русский' }, { k: 'pills', lab: 'Тема', opts: ['Тёмная', 'Светлая'], on: 0 }, { k: 'chk', lab: 'Компактный режим' }] }, { title: 'Время', rows: [{ k: 'dd', lab: 'Часовой пояс', v: 'UTC+0' }, { k: 'chk', lab: '24-часовой формат', on: true }] }] },
        { id: 'perf', label: 'Производительность', groups: [{ title: 'Отрисовка', rows: [{ k: 'dd', lab: 'Частота кадров', v: 'Авто' }, { k: 'chk', lab: 'Сглаживание', on: true }, { k: 'chk', lab: 'Показывать счётчик кадров' }] }, { title: 'Данные', rows: [{ k: 'num', lab: 'Глубина истории, баров', v: 5000 }, { k: 'chk', lab: 'Кешировать на диск', on: true }] }] },
      ],
    },
    {
      id: 'tags_tabs', name: 'Вкладки и метки', icon: 'layers', w: 520, h: 440,
      tabs: [
        { id: 'tabs', label: 'Вкладки', groups: [{ title: 'Вкладки графиков', rows: [{ k: 'table', head: ['Имя', 'Символ', 'ТФ'], rows: [['Untitled 1', 'BTCUSDT', '1H'], ['Скальп', 'ETHUSDT', '5м']] }] }] },
        { id: 'tags', label: 'Метки', groups: [{ title: 'Метки', rows: [{ k: 'pills', opts: ['Идея', 'Работа', 'Архив'], on: 0 }, { k: 'color', lab: 'Цвет метки', v: 'var(--mlc-colors-accent)' }] }, { title: 'Группы синхронизации', rows: [{ k: 'pills', opts: ['Нет', 'Красная', 'Синяя'], on: 1 }] }] },
      ],
    },
    {
      id: 'trading', name: 'Торговый контейнер', icon: 'trading', w: 520, h: 520,
      tabs: [
        { id: 'map', label: 'MAP', groups: [{ title: 'Панели', rows: [{ k: 'pills', opts: ['DOM', 'Лента', 'L2', 'Позиции'], on: 0 }, { k: 'chk', lab: 'Показывать заголовки', on: true }] }] },
        { id: 'setup', label: 'SETUP', groups: [{ title: 'Счёт', rows: [{ k: 'dd', lab: 'Биржа', v: 'BINANCE' }, { k: 'dd', lab: 'Тип', v: 'Spot' }, { k: 'num', lab: 'Плечо', v: 1 }] }, { title: 'Риск', rows: [{ k: 'num', lab: 'Риск на сделку, %', v: '1.0' }, { k: 'chk', lab: 'Стоп обязателен', on: true }] }] },
      ],
    },
    {
      id: 'search', name: 'Поиск инструмента', icon: 'search', w: 520, h: 420,
      tabs: [
        { id: 'symbols', label: 'Инструменты', groups: [{ title: 'Поиск', rows: [{ k: 'area', v: 'BTC' }] }, { title: 'Результаты', rows: [{ k: 'table', head: ['Символ', 'Биржа', 'Тип'], rows: [['BTCUSDT', 'BINANCE', 'spot'], ['BTCUSDT.P', 'BINANCE', 'perp'], ['BTCUSD', 'BYBIT', 'spot']] }] }] },
      ],
    },
    {
      id: 'template_name', name: 'Сохранить шаблон', icon: 'bookmark', w: 380, h: 220,
      tabs: [
        { id: 'name', label: 'Имя', groups: [{ title: 'Название шаблона', rows: [{ k: 'area', v: 'Мой шаблон' }, { k: 'chk', lab: 'Сделать шаблоном по умолчанию' }] }] },
      ],
    },
  ];

  window.SIM_REGISTRY = { TYPES, APP_MODALS, templatesFor, t };
})();
