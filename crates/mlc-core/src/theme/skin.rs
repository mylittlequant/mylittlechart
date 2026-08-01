//! Skin tokens — the 8-color theme model (TapeSurf dissection, owner
//! doctrine 2026-07-31: `docs/mlc/research/tapesurf-ui-dissection-2026-07-30.md` §7).
//!
//! A skin is EIGHT colors: background, text, a (primary, secondary) pair
//! per market side, and a (primary, secondary) accent pair. Everything
//! else — panel surfaces, borders, hovers, three text-dimming tiers,
//! chart colors, per-panel trading colors — is DERIVED by mixing those
//! eight, so one token set restyles every panel and the chart uniformly.
//! The (primary → secondary) side pairs also drive the volume-intensity
//! bar gradients in the orderflow panels (big levels literally glow).
//!
//! `SkinTokens::apply` rewrites the derived subset of an existing
//! [`RuntimeTheme`] in place; fonts, sizing, effects, style are left
//! untouched. Presets registered in [`RuntimeTheme::PRESETS`] build on
//! the dark base theme.

use super::runtime::RuntimeTheme;
use crate::utils::color::{apply_opacity, mix_colors};
use serde::{Deserialize, Serialize};

/// How orderflow volume bars are textured (layer 2 of the skin model:
/// tokens / textures / visualization types).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BarTexture {
    /// Constant side color, length ∝ volume (pre-gradient look).
    Flat,
    /// Color runs side primary → secondary as the level grows (default —
    /// the shipped token-arc look).
    #[default]
    Gradient,
    /// No bar length at all: the whole cell fills at an intensity
    /// proportional to volume (L2 compression). Solo DOM treats it as
    /// Gradient (its cell mode is the `DomSkin::BookmapHybrid` variant).
    Cells,
    /// L3 compression — the TapeSurf "Compact" mosaic (dissection ref 14):
    /// each venue collapses to a narrow strip of small squares, square
    /// SIZE and color intensity ∝ volume, no numbers; per-venue bbo shown
    /// as short horizontal markers across the strip. Multi-DOM only; solo
    /// DOM treats it as Gradient.
    Mosaic,
}

/// How chart candles are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CandleStyle {
    /// Full-width filled bodies (default).
    #[default]
    Solid,
    /// Thin bodies with a soft glow underlay (TapeSurf heatmap-chart
    /// candles).
    ThinGlow,
    /// CRT phosphor: translucent body + bright outline + glow halo.
    Phosphor,
}

/// Skin-driven default renderer variant for the solo DOM ladder (layer 3
/// of the skin model: visualization TYPE defaults). Mirrors
/// `mlc-app-panels::dom::DomSkin`, which lives above mlc-core in the
/// crate graph — the panel maps this at render time; a user's manual
/// per-panel pick always overrides the skin default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DomSkinDefault {
    #[default]
    Classic,
    BidAskSplit,
    BookmapHybrid,
}

/// Skin-driven default renderer variant for the list/tape panels (Trade
/// Tape, Big Trades) — mirrors `mlc-app-panels::trade_tape::TapeSkin`.
/// `Classic` = the original flat-color rows; `Ramp` = TapeSurf
/// trades-feed style (bright-mix intensity ramp + new-row flash). A
/// user's manual per-panel pick always overrides the skin default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TapeSkinDefault {
    #[default]
    Classic,
    Ramp,
}

/// Texture/draw-form parameters of a skin — HOW panels and the chart
/// paint their data, orthogonal to the color tokens. Presets set these
/// together with the tokens; renderers read them from `PanelTheme.style`
/// / `ChartTheme.candle_style`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelStyleParams {
    pub bar_texture: BarTexture,
    /// Soft glow on chart candles (and future glow-capable elements).
    pub glow: bool,
    pub candle_style: CandleStyle,
    /// Zebra-stripe ladder rows (`row_bg_alt`) in DOM/Multi-DOM.
    pub row_zebra: bool,
    /// Default solo-DOM renderer variant for this skin.
    #[serde(default)]
    pub dom_skin: DomSkinDefault,
    /// Default tape-panel renderer variant for this skin.
    #[serde(default)]
    pub tape_skin: TapeSkinDefault,
}

impl Default for PanelStyleParams {
    fn default() -> Self {
        Self {
            bar_texture: BarTexture::Gradient,
            glow: false,
            candle_style: CandleStyle::Solid,
            row_zebra: false,
            dom_skin: DomSkinDefault::Classic,
            tape_skin: TapeSkinDefault::Classic,
        }
    }
}

/// The eight colors a skin is made of. `&'static str` hex values —
/// presets are compile-time constants (runtime-editable skins can layer
/// on later exactly like `RuntimeTheme` layers on `UITheme`).
#[derive(Clone, Copy, Debug)]
pub struct SkinTokens {
    /// Preset name as shown in the theme picker (and matched by
    /// `RuntimeTheme::from_preset`).
    pub name: &'static str,
    pub bg: &'static str,
    pub text: &'static str,
    /// Bid/buy side: base color and the "hot" gradient endpoint that big
    /// volumes lean toward.
    pub bids: &'static str,
    pub bids_secondary: &'static str,
    /// Ask/sell side pair, same semantics.
    pub asks: &'static str,
    pub asks_secondary: &'static str,
    pub accent: &'static str,
    pub accent_secondary: &'static str,
    /// The texture/draw-form layer this skin ships with.
    pub style: PanelStyleParams,
}

impl SkinTokens {
    // ── Derived tiers (mix ratios lifted from the TapeSurf dissection) ──

    /// Panel/input surface: bg pulled 26% toward text.
    pub fn bg_dimmed(&self) -> String {
        mix_colors(self.bg, self.text, 0.26)
    }
    /// Outline/border tier: bg pulled 36% toward text.
    pub fn bg_very_dimmed(&self) -> String {
        mix_colors(self.bg, self.text, 0.36)
    }
    /// Secondary text: text pulled 20% toward bg.
    pub fn text_dimmed(&self) -> String {
        mix_colors(self.text, self.bg, 0.20)
    }
    /// Muted text: 26% toward bg.
    pub fn text_very_dimmed(&self) -> String {
        mix_colors(self.text, self.bg, 0.26)
    }
    /// Faintest text (disabled/axis minor): 36% toward bg.
    pub fn text_super_dimmed(&self) -> String {
        mix_colors(self.text, self.bg, 0.36)
    }
    /// Subtle raised surface (headers, zebra rows): 8% toward text.
    pub fn bg_raised(&self) -> String {
        mix_colors(self.bg, self.text, 0.08)
    }
    /// Hairline (grid, separators): 12% toward text.
    pub fn hairline(&self) -> String {
        mix_colors(self.bg, self.text, 0.12)
    }

    /// Rewrite the derived subset of `rt` from these tokens. Fonts,
    /// sizing, effects and UI style are untouched.
    pub fn apply(&self, rt: &mut RuntimeTheme) {
        rt.name = self.name.to_string();
        rt.panel_style = self.style;
        let bg_dim = self.bg_dimmed();
        let bg_vdim = self.bg_very_dimmed();
        let bg_raised = self.bg_raised();
        let hairline = self.hairline();
        let t_dim = self.text_dimmed();
        let t_vdim = self.text_very_dimmed();
        let t_sdim = self.text_super_dimmed();

        // ── UI chrome ────────────────────────────────────────────────────
        let c = &mut rt.colors;
        c.toolbar_bg = self.bg.to_string();
        c.button_bg = bg_raised.clone();
        c.button_bg_hover = bg_dim.clone();
        c.button_bg_active = mix_colors(self.bg, self.accent, 0.22);
        c.dropdown_bg = mix_colors(self.bg, self.text, 0.04);
        c.button_hover_stroke = bg_vdim.clone();
        c.button_active_stroke = self.accent.to_string();
        c.status_bar_bg = self.bg.to_string();
        c.text_primary = self.text.to_string();
        c.text_secondary = t_dim.clone();
        c.text_muted = t_vdim.clone();
        c.text_active = self.text.to_string();
        c.border = hairline.clone();
        c.border_light = bg_raised.clone();
        c.divider = hairline.clone();
        c.toolbar_divider = hairline.clone();
        c.ui_border = hairline.clone();
        c.accent = self.accent.to_string();
        c.accent_hover = mix_colors(self.accent, self.text, 0.25);
        c.success = self.bids.to_string();
        c.danger = self.asks.to_string();
        c.warning = self.accent.to_string();
        c.text_selection = apply_opacity(self.accent, 0.30);
        c.bubble_user_bg = bg_dim.clone();

        // ── Chart ────────────────────────────────────────────────────────
        let ch = &mut rt.chart;
        ch.background = self.bg.to_string();
        ch.grid_line = mix_colors(self.bg, self.text, 0.06);
        ch.grid_line_horz = None;
        ch.grid_line_vert = None;
        ch.scale_bg = self.bg.to_string();
        ch.scale_border = hairline.clone();
        ch.scale_text = t_dim.clone();
        ch.scale_text_muted = t_sdim.clone();
        ch.time_scale_bg = self.bg.to_string();
        ch.time_scale_border = hairline.clone();
        ch.time_scale_text = t_dim.clone();
        ch.time_scale_text_medium = t_vdim.clone();
        ch.time_scale_text_muted = t_sdim.clone();
        ch.crosshair_line = t_vdim.clone();
        ch.crosshair_label_bg = bg_dim.clone();
        ch.crosshair_label_text = self.text.to_string();
        ch.legend_text = t_dim.clone();
        ch.legend_value_up = self.bids.to_string();
        ch.legend_value_down = self.asks.to_string();
        ch.watermark_text = mix_colors(self.bg, self.text, 0.07);
        ch.sidebar_bg = self.bg.to_string();
        ch.sidebar_border = hairline.clone();
        ch.sidebar_header_bg = bg_raised.clone();
        ch.sidebar_text = t_dim.clone();
        ch.chart_border = hairline.clone();
        ch.frame_border = hairline.clone();

        // ── Series ───────────────────────────────────────────────────────
        let s = &mut rt.series;
        s.candle_up_body = self.bids.to_string();
        s.candle_up_wick = mix_colors(self.bids, self.bg, 0.25);
        s.candle_up_border = None;
        s.candle_down_body = self.asks.to_string();
        s.candle_down_wick = mix_colors(self.asks, self.bg, 0.25);
        s.candle_down_border = None;
        s.line_color = self.accent.to_string();
        s.step_line_color = self.accent.to_string();
        s.line_markers_color = self.accent_secondary.to_string();
        s.area_line = self.accent.to_string();
        s.area_top = apply_opacity(self.accent, 0.25);
        s.area_bottom = apply_opacity(self.accent, 0.02);
        s.histogram_positive = self.bids.to_string();
        s.histogram_negative = self.asks.to_string();
        s.baseline_top_line = self.bids.to_string();
        s.baseline_top_fill = apply_opacity(self.bids, 0.18);
        s.baseline_bottom_line = self.asks.to_string();
        s.baseline_bottom_fill = apply_opacity(self.asks, 0.18);
        s.baseline_line = t_vdim.clone();
        s.bar_up = self.bids.to_string();
        s.bar_down = self.asks.to_string();
        s.ma_fast = self.accent.to_string();
        s.ma_slow = self.accent_secondary.to_string();
        s.ma_third = mix_colors(self.accent, self.text, 0.5);
        // Volume bars carry the side colour at FULL strength. They used to
        // be dimmed to 55%, which read as washed-out next to every other
        // surface the same skin paints — the panel ladders, the candles and
        // now the on-chart heat all use the token undimmed.
        s.volume_up = self.bids.to_string();
        s.volume_down = self.asks.to_string();
        s.heat_bid = self.bids.to_string();
        s.heat_bid_hot = self.bids_secondary.to_string();
        s.heat_ask = self.asks.to_string();
        s.heat_ask_hot = self.asks_secondary.to_string();
        s.footprint_buy = self.bids.to_string();
        s.footprint_sell = self.asks.to_string();
        s.renko_up = self.bids.to_string();
        s.renko_down = self.asks.to_string();
        s.pnf_x = self.bids.to_string();
        s.pnf_o = self.asks.to_string();
        s.tlb_up = self.bids.to_string();
        s.tlb_down = self.asks.to_string();
        s.kagi_yang = self.bids.to_string();
        s.kagi_yin = self.asks.to_string();
        s.hollow_up = self.bids.to_string();
        s.hollow_down = self.asks.to_string();

        // ── Trading panels ───────────────────────────────────────────────
        let tr = &mut rt.trading;
        tr.panel_bg = self.bg.to_string();
        tr.row_bg_alt = mix_colors(self.bg, self.text, 0.03);
        tr.header_bg = bg_raised.clone();
        tr.hover = mix_colors(self.bg, self.text, 0.10);
        tr.selected = mix_colors(self.bg, self.accent, 0.15);
        // The gradient endpoints for volume-intensity bars.
        tr.buy_bright = self.bids_secondary.to_string();
        tr.sell_bright = self.asks_secondary.to_string();
        tr.dom_spread_bg = bg_raised.clone();
        tr.dom_best_bid_bg = mix_colors(self.bg, self.bids, 0.18);
        tr.dom_best_ask_bg = mix_colors(self.bg, self.asks, 0.18);
        tr.dom_user_order = self.accent.to_string();
        tr.fp_cell_text = self.text.to_string();
        tr.fp_poc_marker = self.accent.to_string();
        tr.fp_poc_border = mix_colors(self.accent, self.bg, 0.35);
        tr.fp_bullish = self.bids.to_string();
        tr.vp_bar = apply_opacity(self.bids, 0.45);
        tr.vp_bar_poc = self.accent.to_string();
        tr.vp_poc_line = self.accent.to_string();
        tr.vp_vah_line = apply_opacity(self.asks, 0.5);
        tr.vp_val_line = apply_opacity(self.bids, 0.5);
        tr.vp_value_area = apply_opacity(self.accent, 0.10);
        tr.heatmap_price_line = self.accent.to_string();
        tr.oe_tab_active = self.accent.to_string();
        tr.oe_tab_inactive = bg_dim.clone();
        tr.oe_input_bg = self.bg.to_string();
        tr.oe_input_border = bg_vdim.clone();
        tr.oe_buy_button = self.bids.to_string();
        tr.oe_sell_button = self.asks.to_string();
        // Accent-on-bright buttons read best with the BG as text
        // (TapeSurf accentTextColor pattern).
        tr.oe_buy_button_text = self.bg.to_string();
        tr.oe_sell_button_text = self.bg.to_string();
        tr.pm_pnl_positive = self.bids.to_string();
        tr.pm_pnl_negative = self.asks.to_string();
        tr.pm_pnl_neutral = t_vdim.clone();
        tr.pm_long = self.bids.to_string();
        tr.pm_short = self.asks.to_string();
        tr.pm_liquidation = self.asks_secondary.to_string();
        tr.pm_summary_bg = bg_raised.clone();
        tr.tl_row_bg_alt = bg_raised.clone();
        tr.tl_profit = self.bids.to_string();
        tr.tl_loss = self.asks.to_string();
        tr.rc_risk = self.asks.to_string();
        tr.rc_profit = self.bids.to_string();
        tr.rc_good_rr = self.accent.to_string();
        tr.rc_input_bg = self.bg.to_string();
        tr.rc_input_border = bg_vdim.clone();
        tr.tc_bg = self.bg.to_string();
        tr.tc_inner_bg = mix_colors(self.bg, self.text, 0.05);
        tr.tc_separator = hairline;
    }

    // ── Presets (token values from the TapeSurf dissection table) ────────

    /// The acid look the owner picked: near-black warm bg, cream text,
    /// cyan→green bids, red→amber asks, cyan accent.
    pub const SURF: SkinTokens = SkinTokens {
        name: "surf",
        bg: "#121215",
        text: "#fffcf0",
        bids: "#00ffda",
        bids_secondary: "#30ff76",
        asks: "#ff1919",
        asks_secondary: "#ffd311",
        accent: "#00ffda",
        accent_secondary: "#30ff76",
        style: PanelStyleParams {
            bar_texture: BarTexture::Gradient,
            glow: true,
            candle_style: CandleStyle::ThinGlow,
            row_zebra: false,
            dom_skin: DomSkinDefault::Classic,
            tape_skin: TapeSkinDefault::Ramp,
        },
    };

    /// Deep violet bg, pink→purple asks, blue→cyan bids.
    pub const VAPORWAVE: SkinTokens = SkinTokens {
        name: "vaporwave",
        bg: "#13042c",
        text: "#d3f8ff",
        bids: "#1c77ff",
        bids_secondary: "#1ccaff",
        asks: "#f806bc",
        asks_secondary: "#bc06f8",
        accent: "#1ccaff",
        accent_secondary: "#f806bc",
        style: PanelStyleParams {
            bar_texture: BarTexture::Gradient,
            glow: true,
            candle_style: CandleStyle::ThinGlow,
            row_zebra: false,
            // Neon heat cells — the radical variant for the neon skin.
            dom_skin: DomSkinDefault::BookmapHybrid,
            tape_skin: TapeSkinDefault::Ramp,
        },
    };

    /// CRT: pure black, green phosphor text/bids, red asks.
    pub const TERMINAL: SkinTokens = SkinTokens {
        name: "terminal",
        bg: "#000000",
        text: "#00ff00",
        bids: "#00ff00",
        bids_secondary: "#11af00",
        asks: "#ff0000",
        asks_secondary: "#ff6363",
        accent: "#00ff00",
        accent_secondary: "#ff0000",
        style: PanelStyleParams {
            bar_texture: BarTexture::Flat,
            glow: true,
            candle_style: CandleStyle::Phosphor,
            row_zebra: true,
            dom_skin: DomSkinDefault::Classic,
            tape_skin: TapeSkinDefault::Classic,
        },
    };

    /// All skin presets, in picker order.
    pub const ALL: &'static [SkinTokens] =
        &[Self::SURF, Self::VAPORWAVE, Self::TERMINAL];

    /// Look up a preset by its `name`.
    pub fn by_name(name: &str) -> Option<&'static SkinTokens> {
        Self::ALL.iter().find(|s| s.name == name)
    }
}
