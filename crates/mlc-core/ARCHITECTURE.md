# zengeld-chart Architecture

Документ описывает разделение ответственности между `zengeld-chart` (standalone charting library) и `zengeld-terminal` (trading terminal application).

## Философия

**Chart = Pure Charting**

Chart крейт - это чистая библиотека для рендеринга графиков. Она НЕ содержит:
- UI chrome (toolbars, sidebars, context menus, modals)
- Trading-specific features (indicators, alerts, positions)
- Multi-window management

Если нужен демо UI для showcase - создать отдельный `zengeld-chart-demo` крейт.

## Что удалено из Chart (переехало в Terminal)

### Удаленные типы

| Тип | Был в | Что делать в Terminal |
|-----|-------|----------------------|
| `SidebarState` | `stubs.rs` | Создать свой, конвертировать в `Margins` |
| `ToolbarStyle` | `stubs.rs` | Использовать `RuntimeTheme.sizing` |
| `StyleCatalog` | `stubs.rs` | Использовать `ThemeManager` |
| `ContextMenuState` | `stubs.rs` | Создать свой UI оверлей |
| `ContextMenuTarget` | `stubs.rs` | Создать свой enum |
| `ContextMenuItem` | `stubs.rs` | Создать свою структуру |
| `PrimitiveSettingsState` | `stubs.rs` | Создать свой модальный UI |
| `SelectedPrimitiveConfig` | `stubs.rs` | Получать данные напрямую из `DrawingManager` |
| `WindowLayout` | `stubs.rs` | Создать свой enum в terminal |
| `IndicatorManager` | никогда не было | Создать |
| `AlertManager` | никогда не было | Создать |

### Удаленные методы из DrawingManager

| Метод | Что делать в Terminal |
|-------|----------------------|
| `selected_primitive_config()` | Использовать `selected_primitive()` и читать `.data()` напрямую |
| `handle_toolbar_action()` | Обрабатывать toolbar actions в terminal напрямую |

### Удаленные ChartAction варианты

| Action | Что делать в Terminal |
|--------|----------------------|
| `SetWindowLayout(WindowLayout)` | Создать свой enum `TerminalAction` |
| `ToggleSyncSymbol` | Создать свой enum |
| `ToggleSyncTimeframe` | Создать свой enum |
| `ToggleSyncCrosshair` | Создать свой enum |
| `ToggleSyncViewport` | Создать свой enum |

### Удаленные функции рендеринга

| Функция | Была в | Что делать в Terminal |
|---------|--------|----------------------|
| `draw_indicator_line` | `render_chart.rs` | Реализовать самим |
| `draw_indicator_band` | `render_chart.rs` | Реализовать самим |
| `draw_volume_overlay` | `render_chart.rs` | Реализовать самим |
| `draw_overlay_indicators` | `render_chart.rs` | Реализовать самим |
| `draw_alert_lines` | `render_chart.rs` | Реализовать самим |
| `draw_sub_pane_line` | `render_chart.rs` | Реализовать самим |
| `draw_sub_pane_histogram` | `render_chart.rs` | Реализовать самим |
| `draw_toolbar_background` | `render_chart.rs` | Terminal рисует сам |
| `draw_toolbar_backgrounds` | `render_chart.rs` | Terminal рисует сам |
| `draw_toolbar_border` | `render_chart.rs` | Terminal рисует сам |

### Изменения в Layout

**Было (старый API):**
```rust
// FrameLayout содержал toolbar rects
pub struct FrameLayout {
    pub total: LayoutRect,
    pub top_toolbar: LayoutRect,      // УДАЛЕНО
    pub left_toolbar: LayoutRect,     // УДАЛЕНО
    pub right_toolbar: LayoutRect,    // УДАЛЕНО
    pub bottom_toolbar: LayoutRect,   // УДАЛЕНО
    pub right_sidebar: LayoutRect,    // УДАЛЕНО
    pub bottom_sidebar: LayoutRect,   // УДАЛЕНО
    pub chart_area: ChartAreaLayout,
    pub chart_panel: LayoutRect,
}

// Создавался через SidebarState
let layout = FrameLayout::compute(width, height, &sidebar_state);
```

**Стало (новый API):**
```rust
// FrameLayout содержит только chart-related поля
pub struct FrameLayout {
    pub total: LayoutRect,
    pub chart_panel: LayoutRect,
    pub chart_area: ChartAreaLayout,
}

// Создается через Margins
let margins = Margins::new(top, left, right, bottom);
let layout = FrameLayout::compute(width, height, &margins);
```

### Изменения в Theme

**Было:**
```rust
// icon_size брался из StyleCatalog
let icon_size = StyleCatalog::toolbar().icon_size;
```

**Стало:**
```rust
// icon_size в UISizing / RuntimeSizing
pub struct UISizing {
    pub icon_size: f32,  // ДОБАВЛЕНО
    // ...
}
```

## Что осталось в Chart

### Stubs модуль

```rust
// stubs.rs - сейчас пустой
// Chart - чистая библиотека без UI stubs
// Для demo UI используйте zengeld-chart-demo
```

### Core Rendering (chart предоставляет)

```rust
// Основной рендеринг
render_chart_window(ctx, &layout.chart_area, state, config, corner_state) -> ScaleCornerHitZones
render_chart(ctx, layout, state, config)  // только chart area без scales
render_scales(ctx, layout, state, config)

// Sub-pane base (без индикаторов)
render_sub_pane_base(ctx, pane_layout, pane_index, state, min, max, title, ...)
render_sub_pane_primitives(ctx, content, state, dm, instance_id, min, max)

// Отдельные элементы
draw_grid(ctx, state)
draw_candles(ctx, state)
draw_price_scale(ctx, state, config, theme, x, y)
draw_time_scale(ctx, state, config, theme, x, y)
draw_crosshair(ctx, state, config, is_dragging, top, bottom)
draw_legend(ctx, rect, legend, data, up_color, down_color)
draw_tooltip(ctx, rect, tooltip, content, x, y)
```

### DrawingManager API (для UI integration)

```rust
// Получение данных о выбранном примитиве (вместо selected_primitive_config)
if let Some(prim) = drawing_manager.selected_primitive() {
    let data = prim.data();
    let name = prim.display_name();
    let color = &data.color.stroke;
    let width = data.width;
    let locked = data.locked;
    let style = data.style;

    // Text info
    let text_color = data.text.as_ref().and_then(|t| t.color.clone());

    // Check if type supports text
    let registry = PrimitiveRegistry::global().read().unwrap();
    let supports_text = registry.supports_text(prim.type_id());
}

// Действия (вместо handle_toolbar_action)
drawing_manager.set_selected_color("#ff0000");
drawing_manager.set_selected_width(2.0);
drawing_manager.set_selected_style(LineStyle::Dashed);
drawing_manager.toggle_selected_lock();
drawing_manager.delete_selected();
```

### Layout Types

```rust
// Margins - terminal передает сколько места занято UI
pub struct Margins {
    pub top: f64,     // toolbar height
    pub left: f64,    // left toolbar width
    pub right: f64,   // right toolbar + sidebar
    pub bottom: f64,  // bottom toolbar + panel
}

// FrameLayout - chart вычисляет свой layout
pub struct FrameLayout {
    pub total: LayoutRect,
    pub chart_panel: LayoutRect,
    pub chart_area: ChartAreaLayout,
}

// ChartAreaLayout - subdivision chart area
pub struct ChartAreaLayout {
    pub chart: LayoutRect,
    pub price_scale: LayoutRect,
    pub time_scale: LayoutRect,
    pub scale_corner: LayoutRect,
}

// ExtendedFrameLayout - с sub-panes
pub struct ExtendedFrameLayout {
    pub main_chart: ChartAreaLayout,
    pub sub_panes: Vec<SubPaneLayout>,
    pub frame: FrameLayout,
}

// SubPaneLayout - geometry для indicator pane
pub struct SubPaneLayout {
    pub content: LayoutRect,
    pub price_scale: LayoutRect,
    pub separator: LayoutRect,
    pub instance_id: u64,
}
```

### Demo Data

```rust
// demo/symbols.rs
pub enum SymbolCategory { Stock, Crypto, Forex, Index, Futures, Options }
pub struct SymbolInfo { symbol, name, category, exchange, popular }
pub struct DemoSymbol { info, base_price, volatility, trend_bias, seed_offset, precision }
pub fn demo_symbols() -> Vec<DemoSymbol>
pub fn get_demo_symbol(ticker: &str) -> Option<DemoSymbol>
```

## Terminal Integration Guide

### 1. Layout Integration

```rust
// Terminal создает свой SidebarState
pub struct SidebarState {
    pub left_open: bool,
    pub right_open: bool,
    pub bottom_open: bool,
    pub left_width: f64,
    pub right_width: f64,
    pub bottom_height: f64,
}

impl SidebarState {
    pub fn to_margins(&self, toolbar_heights: &ToolbarHeights) -> Margins {
        Margins {
            top: toolbar_heights.top,
            left: toolbar_heights.left,
            right: toolbar_heights.right + if self.right_open { self.right_width } else { 0.0 },
            bottom: toolbar_heights.bottom + if self.bottom_open { self.bottom_height } else { 0.0 },
        }
    }
}
```

### 2. Rendering Frame

```rust
fn render_frame(ctx: &mut dyn RenderContext, ...) {
    // 1. Compute margins
    let margins = sidebar.to_margins(&toolbar_heights);

    // 2. Get chart layout
    let layout = FrameLayout::compute(window_width, window_height, &margins);

    // 3. Terminal рисует свои toolbars (chart не знает о них)
    ctx.set_fill_color(&theme.toolbar_bg);
    ctx.fill_rect(0.0, 0.0, window_width, toolbar_heights.top);  // top toolbar
    ctx.fill_rect(0.0, 0.0, toolbar_heights.left, window_height); // left toolbar
    // ... и т.д.

    // 4. Chart рисует chart area
    render_chart_window(ctx, &layout.chart_area, state, config, corner_state);

    // 5. Terminal рисует indicators поверх chart
    draw_overlay_indicators(ctx, state, indicator_manager);

    // 6. Terminal рисует alerts
    draw_alert_lines(ctx, state, alert_items);

    // 7. Sub-panes (если есть)
    let extended = ExtendedFrameLayout::compute(...);
    for (i, pane) in extended.sub_panes.iter().enumerate() {
        // Chart рисует base (background, grid, scale, crosshair)
        render_sub_pane_base(ctx, pane, i, state, min, max, title, ...);

        // Terminal рисует indicator content
        draw_sub_pane_indicator(ctx, state, pane, indicator_values, ...);

        // Chart рисует primitives в этом pane
        render_sub_pane_primitives(ctx, &pane.content, state, dm, pane.instance_id, min, max);
    }

    // 8. Terminal рисует sidebars если открыты
    if sidebar.right_open {
        draw_sidebar(ctx, sidebar_rect, ...);
    }
}
```

### 3. Indicator Rendering (Terminal implements)

```rust
// Terminal реализует эти функции
pub fn draw_indicator_line(
    ctx: &mut dyn RenderContext,
    viewport: &Viewport,
    price_scale: &PriceScale,
    chart_rect: &LayoutRect,
    values: &[f64],
    color: &str,
    line_width: f32,
) {
    let (start, end) = viewport.visible_range();

    ctx.set_stroke_color(color);
    ctx.set_stroke_width(line_width);
    ctx.begin_path();

    let mut first = true;
    for i in start..=end.min(values.len().saturating_sub(1)) {
        let x = viewport.bar_to_x(i) + chart_rect.x;
        let y = price_scale.price_to_y(values[i], chart_rect.height) + chart_rect.y;

        if first {
            ctx.move_to(x, y);
            first = false;
        } else {
            ctx.line_to(x, y);
        }
    }
    ctx.stroke();
}
```

### 4. Coordinate Helpers (Chart provides)

```rust
// Viewport helpers
viewport.bar_to_x(bar_index: usize) -> f64
viewport.bar_to_x_f64(bar: f64) -> f64
viewport.x_to_bar(x: f64) -> f64
viewport.visible_range() -> (usize, usize)

// PriceScale helpers
price_scale.price_to_y(price: f64, height: f64) -> f64
price_scale.y_to_price(y: f64, height: f64) -> f64
```

## ChartAction Notes

`ChartAction` enum содержит actions только для chart:

**Chart handles:**
- `SetChartType`, `ToggleLegend`, `ToggleTooltip`, `ToggleGrid`
- `ResetZoom`, `FitContent`, `ZoomIn`, `ZoomOut`
- `SelectTool`, `DeleteSelected`, `DeleteAll`
- `Undo`, `Redo`
- `SetTheme`, `SetStyle`

**Terminal создает свой enum для:**
- Multi-window: `SetWindowLayout`, `ToggleSyncSymbol`, etc.
- Sidebars: `ToggleWatchlist`, `ToggleAlerts`, etc.
- Trading: `ToggleTradingPanel`, `TogglePositions`
- Modals: `OpenIndicators`, `OpenSettings`

## Internationalization (i18n)

### Философия

Chart использует собственную **zero-dependency** систему i18n:
- Английский язык — базовый (все ID на английском)
- Поддержка runtime-переключения языка (thread-safe через `AtomicU8`)
- Compile-time проверка всех ключей через типизированные enums
- Статические строки (`&'static str`) — без heap allocation

### Структура i18n модуля

```
crates/chart/src/i18n/
├── mod.rs          # Language enum, global state, helper functions
├── keys.rs         # Typed translation keys (ConfigKey, TextKey, etc.)
└── translations.rs # Translatable trait
```

### Типы ключей переводов

| Enum | Назначение |
|------|------------|
| `ConfigKey` | Свойства примитивов (Labels, Prices, Levels, ExtendLeft, etc.) |
| `TextKey` | Общие UI тексты |
| `MenuKey` | Пункты меню |
| `WaveDegreeKey` | Elliott Wave степени (Subminuette, Minuette, Cycle, etc.) |
| `StyleKey` | Стили линий (Standard, Dashed, Dotted, etc.) |
| `LabelPositionKey` | Позиции меток (Left, Right, Center) |
| `MonthKey` | Названия месяцев |

### Использование

```rust
use zengeld_chart::i18n::{Language, set_language, current_language, ConfigKey};

// Установить язык глобально
set_language(Language::Ru);

// Получить перевод по ключу
let label = ConfigKey::ShowLabels.get(current_language()); // "Показывать метки"

// Хелпер функции
use zengeld_chart::{t_config, t_wave};
let text = t_config(ConfigKey::Prices);     // "Prices" / "Цены"
let wave = t_wave(WaveDegreeKey::Cycle);    // "Cycle" / "Цикл"
```

### ConfigProperty Helpers

Для примитивов используются типизированные хелперы вместо hardcoded строк:

```rust
// Было (hardcoded Russian):
ConfigProperty::boolean("show_labels", "Показывать метки", value)

// Стало (i18n):
ConfigProperty::show_labels(value)  // автоматически использует current_language()
```

Доступные хелперы в `ConfigProperty`:
- `show_labels()`, `show_lines()`, `show_ratios()`, `show_prices()`, `show_levels()`
- `show_price()`, `show_line()`, `show_header()`, `show_neckline()`, `show_background()`
- `extend_left()`, `extend_right()`, `extend_lines()`, `show_trend_line()`
- `label_position()`, `label_font_size()`, `label_color()`
- `font_size()`, `text_color()`, `bold()`, `italic()`
- `triangle_type()`, `direction()`, `level_mode()`, `wave_degree()`
- `h_align()`, `v_align()`, `rows_count()`, `columns_count()`
- и другие...

### Добавление новых переводов

1. Добавить вариант в enum `ConfigKey` в `keys.rs`
2. Добавить English перевод в `fn en()`
3. Добавить Russian перевод в `fn ru()`
4. (Опционально) Создать хелпер в `ConfigProperty`

```rust
// keys.rs
pub enum ConfigKey {
    // ...
    MyNewKey,
}

impl ConfigKey {
    fn en(self) -> &'static str {
        match self {
            // ...
            Self::MyNewKey => "My New Setting",
        }
    }

    fn ru(self) -> &'static str {
        match self {
            // ...
            Self::MyNewKey => "Моя новая настройка",
        }
    }
}
```

## Theme System

### Философия

Chart использует **self-contained** тематизацию:
- Только цвета и шрифты, относящиеся к графику
- Терминальные элементы (toolbar, button, modal) — ответственность терминала
- StyleParams оставлен для opacity subpanes и эффектов

### Типы тем

| Тип | Назначение | Рекомендация |
|-----|------------|--------------|
| `ChartTheme` | Полная чартовая тема (colors + series + fonts) | **Использовать для нового кода** |
| `ChartColors` | Background, grid, scales, crosshair, legend, watermark | Chart-specific |
| `SeriesColors` | Candles, line, area, histogram, baseline, bars, volume | Chart-specific |
| `ChartFonts` | Price scale, time scale, legend, crosshair, watermark | Chart-specific |
| `UITheme` | Полная UI тема (включая terminal) | **Legacy — терминал должен мигрировать** |
| `UIColors` | Toolbar, buttons, dropdown, status bar | Terminal-specific |
| `UISizing` | Toolbar dimensions, button sizes | Terminal-specific |
| `UIEffects` | Transitions, shadows, hover scale | Terminal-specific |
| `StyleParams` | Opacity, blur, subpane backgrounds | **Оставлен для chart** |

### Chart-Specific Theme (рекомендуется)

```rust
use zengeld_chart::{ChartTheme, ChartColors, SeriesColors, ChartFonts};

// Использование preset
let theme = ChartTheme::dark();

// Доступ к цветам
let bg = theme.chart.background;
let candle_up = theme.series.candle_up_body;
let font = theme.fonts.family;

// Хелперы
let font_str = theme.price_scale_font(12.0); // "12px 'Trebuchet MS', Arial, sans-serif"
let grid_color = theme.grid_color(true);     // Horizontal grid line color
```

### Legacy UI Theme (для терминала)

```rust
use zengeld_chart::{UITheme, UIColors, UISizing, UIEffects};

// UITheme содержит всё — и chart и terminal
let theme = UITheme::dark();

// Terminal-specific (должны мигрировать в terminal crate)
let toolbar_bg = theme.colors.toolbar_bg;
let button_hover = theme.colors.button_bg_hover;
let icon_size = theme.sizing.icon_size;
```

### Миграция терминала

Terminal должен создать свой `TerminalTheme`:

```rust
// В terminal crate (будущее)
pub struct TerminalTheme {
    /// Chart theming (from zengeld-chart)
    pub chart: ChartTheme,

    /// Terminal UI (собственные типы терминала)
    pub ui: TerminalUIColors,
    pub sizing: TerminalSizing,
    pub effects: TerminalEffects,

    /// Style (opacity, blur — используется и chart и terminal)
    pub style: UIStyle,
    pub style_params: StyleParams,
}

impl TerminalTheme {
    pub fn dark() -> Self {
        Self {
            chart: ChartTheme::dark(),
            ui: TerminalUIColors::dark(),
            // ...
        }
    }
}
```

### StyleParams (оставлен для chart)

StyleParams используется для:
- `sub_pane_bg_opacity` — прозрачность фона indicator pane
- `scale_bg_opacity` — прозрачность фона шкал
- `chart_border`, `frame_border` — стили рамок
- Glass effects (blur, refraction) — для sub-pane backgrounds

```rust
use zengeld_chart::{StyleParams, OpacityType};

let params = StyleParams::default();
let subpane_opacity = params.sub_pane_bg_opacity;
```

### Пресеты

Доступные пресеты (и `ChartTheme`, и `UITheme`):
- `dark()` — TradingView-style темная тема
- `light()` — Светлая тема
- `high_contrast()` — Accessibility высокий контраст
- `cyberpunk()` — Neon/cyberpunk стиль

## File Structure

```
crates/chart/src/
├── lib.rs                 # Main exports
├── stubs.rs               # Empty (UI stubs removed)
├── i18n/                  # Internationalization
│   ├── mod.rs             # Language enum, global state
│   ├── keys.rs            # ConfigKey, TextKey, MenuKey, etc.
│   └── translations.rs    # Translatable trait
├── chart/
│   ├── render/            # Core rendering (candles, grid, scales, crosshair)
│   └── types/             # Bar, Viewport, PriceScale, Crosshair, etc.
├── drawing/
│   ├── manager.rs         # DrawingManager
│   └── primitives_v2/     # 100+ primitive types
├── layout/
│   ├── compute.rs         # Margins, LayoutConfig, FrameLayout::compute()
│   ├── rects.rs           # LayoutRect, ChartAreaLayout, SubPaneLayout
│   ├── hit_tester.rs      # Hit testing
│   └── render_chart.rs    # High-level render functions
├── state/
│   └── chart.rs           # Base Chart struct
├── theme/
│   ├── mod.rs             # Module exports (ChartTheme + legacy UITheme)
│   ├── preset.rs          # ChartTheme, UITheme presets (dark, light, etc.)
│   ├── runtime.rs         # RuntimeTheme (modifiable at runtime)
│   ├── manager.rs         # ThemeManager
│   └── style.rs           # UIStyle, StyleParams (opacity, blur effects)
└── demo/
    ├── symbols.rs         # SymbolCategory, SymbolInfo, DemoSymbol
    └── data_generator.rs  # Demo bar generation
```
