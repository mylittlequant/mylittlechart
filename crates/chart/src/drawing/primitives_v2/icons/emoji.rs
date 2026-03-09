//! Emoji primitive - emoji marker
//!
//! Since emoji fonts are not reliably available in all canvas/GPU contexts,
//! we render SVG-style vector icons for common markers.
//!
//! Uses 5 data-coordinate points: center + 4 edge points (top, right, bottom, left)

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, ControlPointCursor, PrimitiveColor,
    RenderContext, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text, TextAlign,
};

/// Available emoji/icon types
/// Grouped by category:
/// - Signals: Trading-related signals (buy/sell, entry/exit, etc.)
/// - Markers: General purpose markers (target, flag, star, etc.)
/// - Emotions: Emotional/sentiment indicators
/// - Arrows: Directional indicators
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmojiType {
    // === Signals (Trading) ===
    Target,       // 🎯 Price target
    Flag,         // 🚩 Flag marker
    Check,        // ✓ Confirmed/done
    Cross,        // ✗ Rejected/stop
    Warning,      // ⚠ Warning/caution
    Dollar,       // $ Money/profit
    Lightning,    // ⚡ Fast/momentum
    Lock,         // 🔒 Locked/secured
    Unlock,       // 🔓 Unlocked/released
    Bell,         // 🔔 Alert
    Eye,          // 👁 Watch/observe
    Clock,        // ⏰ Time-based

    // === Markers ===
    Star,         // ⭐ Important
    Heart,        // ❤ Favorite
    Circle,       // ● Point marker
    Diamond,      // ◆ Diamond marker
    Square,       // ■ Square marker
    Triangle,     // ▲ Triangle marker
    Plus,         // + Add/positive
    Minus,        // - Remove/negative
    Question,     // ? Uncertain
    Info,         // ℹ Information

    // === Emotions ===
    ThumbsUp,     // 👍 Bullish/good
    ThumbsDown,   // 👎 Bearish/bad
    Fire,         // 🔥 Hot/trending
    Rocket,       // 🚀 Moon/rally
    Skull,        // 💀 Dead/crashed
    Crown,        // 👑 King/winner
    Gem,          // 💎 Diamond hands
    Poop,         // 💩 Bad trade
    Frogger,      // 🐸 Cute frog (easter egg)
    Frog,         // 🐸 Frog with top hat

    // === Arrows ===
    ArrowUp,      // ↑ Up movement
    ArrowDown,    // ↓ Down movement
    ArrowLeft,    // ← Left/back
    ArrowRight,   // → Right/forward
}

impl EmojiType {
    pub fn all() -> &'static [EmojiType] {
        &[
            // Signals
            EmojiType::Target,
            EmojiType::Flag,
            EmojiType::Check,
            EmojiType::Cross,
            EmojiType::Warning,
            EmojiType::Dollar,
            EmojiType::Lightning,
            EmojiType::Lock,
            EmojiType::Unlock,
            EmojiType::Bell,
            EmojiType::Eye,
            EmojiType::Clock,
            // Markers
            EmojiType::Star,
            EmojiType::Heart,
            EmojiType::Circle,
            EmojiType::Diamond,
            EmojiType::Square,
            EmojiType::Triangle,
            EmojiType::Plus,
            EmojiType::Minus,
            EmojiType::Question,
            EmojiType::Info,
            // Emotions
            EmojiType::ThumbsUp,
            EmojiType::ThumbsDown,
            EmojiType::Fire,
            EmojiType::Rocket,
            EmojiType::Skull,
            EmojiType::Crown,
            EmojiType::Gem,
            EmojiType::Poop,
            EmojiType::Frogger,
            EmojiType::Frog,
            // Arrows
            EmojiType::ArrowUp,
            EmojiType::ArrowDown,
            EmojiType::ArrowLeft,
            EmojiType::ArrowRight,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            // Signals
            EmojiType::Target => "Target",
            EmojiType::Flag => "Flag",
            EmojiType::Check => "Check",
            EmojiType::Cross => "Cross",
            EmojiType::Warning => "Warning",
            EmojiType::Dollar => "Dollar",
            EmojiType::Lightning => "Lightning",
            EmojiType::Lock => "Lock",
            EmojiType::Unlock => "Unlock",
            EmojiType::Bell => "Bell",
            EmojiType::Eye => "Eye",
            EmojiType::Clock => "Clock",
            // Markers
            EmojiType::Star => "Star",
            EmojiType::Heart => "Heart",
            EmojiType::Circle => "Circle",
            EmojiType::Diamond => "Diamond",
            EmojiType::Square => "Square",
            EmojiType::Triangle => "Triangle",
            EmojiType::Plus => "Plus",
            EmojiType::Minus => "Minus",
            EmojiType::Question => "Question",
            EmojiType::Info => "Info",
            // Emotions
            EmojiType::ThumbsUp => "Thumbs Up",
            EmojiType::ThumbsDown => "Thumbs Down",
            EmojiType::Fire => "Fire",
            EmojiType::Rocket => "Rocket",
            EmojiType::Skull => "Skull",
            EmojiType::Crown => "Crown",
            EmojiType::Gem => "Gem",
            EmojiType::Poop => "Poop",
            EmojiType::Frogger => "Frogger",
            EmojiType::Frog => "Frog",
            // Arrows
            EmojiType::ArrowUp => "Arrow Up",
            EmojiType::ArrowDown => "Arrow Down",
            EmojiType::ArrowLeft => "Arrow Left",
            EmojiType::ArrowRight => "Arrow Right",
        }
    }

    /// Get SVG icon for this emoji type (for toolbar icons)
    pub fn svg(&self) -> &'static str {
        match self {
            // Signals
            EmojiType::Target => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><circle cx="12" cy="12" r="6"/><circle cx="12" cy="12" r="2" fill="currentColor"/><line x1="2" y1="12" x2="6" y2="12"/><line x1="18" y1="12" x2="22" y2="12"/><line x1="12" y1="2" x2="12" y2="6"/><line x1="12" y1="18" x2="12" y2="22"/></svg>"#,
            EmojiType::Flag => r#"<svg viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="2"><line x1="5" y1="22" x2="5" y2="2"/><polygon points="5,2 19,7 5,12"/></svg>"#,
            EmojiType::Check => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><polyline points="4,12 9,17 20,6"/></svg>"#,
            EmojiType::Cross => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><line x1="5" y1="5" x2="19" y2="19"/><line x1="19" y1="5" x2="5" y2="19"/></svg>"#,
            EmojiType::Warning => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="12,2 22,20 2,20"/><line x1="12" y1="9" x2="12" y2="13"/><circle cx="12" cy="17" r="1" fill="currentColor"/></svg>"#,
            EmojiType::Dollar => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="12" y1="1" x2="12" y2="23"/><path d="M17 5H9.5a3.5 3.5 0 0 0 0 7h5a3.5 3.5 0 0 1 0 7H6"/></svg>"#,
            EmojiType::Lightning => r#"<svg viewBox="0 0 24 24" fill="currentColor"><polygon points="13,2 3,14 12,14 11,22 21,10 12,10"/></svg>"#,
            EmojiType::Lock => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="11" width="18" height="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>"#,
            EmojiType::Unlock => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="11" width="18" height="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 9.9-1"/></svg>"#,
            EmojiType::Bell => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9"/><path d="M13.73 21a2 2 0 0 1-3.46 0"/></svg>"#,
            EmojiType::Eye => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></svg>"#,
            EmojiType::Clock => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><polyline points="12,6 12,12 16,14"/></svg>"#,
            // Markers
            EmojiType::Star => r#"<svg viewBox="0 0 24 24" fill="currentColor"><polygon points="12,2 15,9 22,9 17,14 19,22 12,17 5,22 7,14 2,9 9,9"/></svg>"#,
            EmojiType::Heart => r#"<svg viewBox="0 0 24 24" fill="currentColor"><path d="M12,21 C12,21 3,14 3,8.5 C3,5 6,2 9,2 C10.5,2 12,3 12,3 C12,3 13.5,2 15,2 C18,2 21,5 21,8.5 C21,14 12,21 12,21 Z"/></svg>"#,
            EmojiType::Circle => r#"<svg viewBox="0 0 24 24" fill="currentColor"><circle cx="12" cy="12" r="10"/></svg>"#,
            EmojiType::Diamond => r#"<svg viewBox="0 0 24 24" fill="currentColor"><polygon points="12,2 22,12 12,22 2,12"/></svg>"#,
            EmojiType::Square => r#"<svg viewBox="0 0 24 24" fill="currentColor"><rect x="3" y="3" width="18" height="18"/></svg>"#,
            EmojiType::Triangle => r#"<svg viewBox="0 0 24 24" fill="currentColor"><polygon points="12,2 22,22 2,22"/></svg>"#,
            EmojiType::Plus => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>"#,
            EmojiType::Minus => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><line x1="5" y1="12" x2="19" y2="12"/></svg>"#,
            EmojiType::Question => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3"/><circle cx="12" cy="17" r="1" fill="currentColor"/></svg>"#,
            EmojiType::Info => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><circle cx="12" cy="8" r="1" fill="currentColor"/></svg>"#,
            // Emotions
            EmojiType::ThumbsUp => r#"<svg viewBox="0 0 24 24" fill="currentColor"><path d="M14 9V5a3 3 0 0 0-3-3l-4 9v11h11.28a2 2 0 0 0 2-1.7l1.38-9a2 2 0 0 0-2-2.3H14zM7 22H4a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2h3"/></svg>"#,
            EmojiType::ThumbsDown => r#"<svg viewBox="0 0 24 24" fill="currentColor"><path d="M10 15v4a3 3 0 0 0 3 3l4-9V2H5.72a2 2 0 0 0-2 1.7l-1.38 9a2 2 0 0 0 2 2.3H10zm7-13h2.67A2.31 2.31 0 0 1 22 4v7a2.31 2.31 0 0 1-2.33 2H17"/></svg>"#,
            EmojiType::Fire => r#"<svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 23c-4 0-7-3-7-7 0-3 2-5 3-7 0 2 1 3 2 3 0-3 1-6 4-10 0 4 2 6 3 6 0-1 1-2 2-3 2 3 3 5 3 7 0 6-4 11-10 11z"/><path d="M12 23c-2 0-4-2-4-5 0-2 1-3 2-4 0 1 1 2 2 2 0-2 1-4 2-6 1 2 2 4 2 5 0 4-2 8-4 8z" fill="orange"/></svg>"#,
            EmojiType::Rocket => r#"<svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 1 Q17 6 16 15 L8 15 Q7 6 12 1 Z"/><path d="M8 13 L5 18 L8 15 Z"/><path d="M16 13 L19 18 L16 15 Z"/><path d="M10 15 L12 22 L14 15 Z"/></svg>"#,
            EmojiType::Skull => r#"<svg viewBox="0 0 24 24" fill="currentColor"><ellipse cx="12" cy="10" rx="8" ry="7"/><ellipse cx="9" cy="9" rx="2" ry="2" fill="black"/><ellipse cx="15" cy="9" rx="2" ry="2" fill="black"/><rect x="7" y="15" width="10" height="5"/><line x1="9" y1="15" x2="9" y2="20" stroke="black" stroke-width="1.5"/><line x1="11" y1="15" x2="11" y2="20" stroke="black" stroke-width="1.5"/><line x1="13" y1="15" x2="13" y2="20" stroke="black" stroke-width="1.5"/><line x1="15" y1="15" x2="15" y2="20" stroke="black" stroke-width="1.5"/></svg>"#,
            EmojiType::Crown => r#"<svg viewBox="0 0 24 24" fill="currentColor"><path d="M5 16L3 5l5.5 5L12 4l3.5 6L21 5l-2 11H5zm14 3c0 .55-.45 1-1 1H6c-.55 0-1-.45-1-1v-1h14v1z"/></svg>"#,
            EmojiType::Gem => r#"<svg viewBox="0 0 24 24" fill="currentColor"><polygon points="12,2 2,9 12,22 22,9"/><line x1="2" y1="9" x2="22" y2="9" stroke="currentColor" stroke-width="1"/><line x1="12" y1="2" x2="7" y2="9" stroke="currentColor" stroke-width="1"/><line x1="12" y1="2" x2="17" y2="9" stroke="currentColor" stroke-width="1"/><line x1="12" y1="22" x2="7" y2="9" stroke="currentColor" stroke-width="1"/><line x1="12" y1="22" x2="17" y2="9" stroke="currentColor" stroke-width="1"/></svg>"#,
            EmojiType::Poop => r#"<svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 2c-2 0-3 1-3 2 0 .55.22 1.05.58 1.41C8.06 6.06 7 7.52 7 9c0 1.1.45 2.1 1.17 2.83C6.84 12.66 6 14.25 6 16c0 3.31 2.69 6 6 6s6-2.69 6-6c0-1.75-.84-3.34-2.17-4.17C16.55 11.1 17 10.1 17 9c0-1.48-1.06-2.94-2.58-3.59.36-.36.58-.86.58-1.41 0-1-1-2-3-2z"/><circle cx="9" cy="15" r="1"/><circle cx="15" cy="15" r="1"/><path d="M8.5 18c.83 1.2 2.08 2 3.5 2s2.67-.8 3.5-2h-7z"/></svg>"#,
            EmojiType::Frogger => r#"<svg viewBox="0 0 24 24" fill="currentColor"><ellipse cx="12" cy="13" rx="10" ry="8" fill="olivedrab"/><ellipse cx="7" cy="10" rx="4" ry="3" fill="olivedrab"/><ellipse cx="17" cy="10" rx="4" ry="3" fill="olivedrab"/><ellipse cx="7" cy="10" rx="2.5" ry="2" fill="white"/><ellipse cx="17" cy="10" rx="2.5" ry="2" fill="white"/><circle cx="7.5" cy="10" r="1" fill="black"/><circle cx="17.5" cy="10" r="1" fill="black"/><path d="M6 16 Q12 20 18 16" stroke="darkolivegreen" stroke-width="1.5" fill="none"/><ellipse cx="5" cy="15" rx="2" ry="1" fill="lightpink"/><ellipse cx="19" cy="15" rx="2" ry="1" fill="lightpink"/></svg>"#,
            EmojiType::Frog => r#"<svg viewBox="0 0 24 24" fill="currentColor"><rect x="8" y="1" width="8" height="6" fill="black"/><rect x="5" y="6" width="14" height="2" fill="black"/><path d="M4 12c0-4 3-6 8-6s8 2 8 6c0 4-2 7-4 8v1h-8v-1c-2-1-4-4-4-8z" fill="forestgreen"/><ellipse cx="8" cy="11" rx="3" ry="2.5" fill="white"/><ellipse cx="16" cy="11" rx="3" ry="2.5" fill="white"/><ellipse cx="8" cy="9.5" rx="3.2" ry="1.5" fill="forestgreen"/><ellipse cx="16" cy="9.5" rx="3.2" ry="1.5" fill="forestgreen"/><circle cx="9" cy="11.5" r="1.5" fill="black"/><circle cx="17" cy="11.5" r="1.5" fill="black"/><path d="M7 16 Q12 19 17 16" stroke="darkgreen" stroke-width="2" fill="none"/></svg>"#,
            // Arrows
            EmojiType::ArrowUp => r#"<svg viewBox="0 0 24 24" fill="currentColor"><polygon points="12,2 6,10 9,10 9,22 15,22 15,10 18,10"/></svg>"#,
            EmojiType::ArrowDown => r#"<svg viewBox="0 0 24 24" fill="currentColor"><polygon points="12,22 6,14 9,14 9,2 15,2 15,14 18,14"/></svg>"#,
            EmojiType::ArrowLeft => r#"<svg viewBox="0 0 24 24" fill="currentColor"><polygon points="2,12 10,6 10,9 22,9 22,15 10,15 10,18"/></svg>"#,
            EmojiType::ArrowRight => r#"<svg viewBox="0 0 24 24" fill="currentColor"><polygon points="22,12 14,6 14,9 2,9 2,15 14,15 14,18"/></svg>"#,
        }
    }

    /// Get emoji type from string id
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            // Signals
            "target" => Some(EmojiType::Target),
            "flag" => Some(EmojiType::Flag),
            "check" => Some(EmojiType::Check),
            "cross" => Some(EmojiType::Cross),
            "warning" => Some(EmojiType::Warning),
            "dollar" => Some(EmojiType::Dollar),
            "lightning" => Some(EmojiType::Lightning),
            "lock" => Some(EmojiType::Lock),
            "unlock" => Some(EmojiType::Unlock),
            "bell" => Some(EmojiType::Bell),
            "eye" => Some(EmojiType::Eye),
            "clock" => Some(EmojiType::Clock),
            // Markers
            "star" => Some(EmojiType::Star),
            "heart" => Some(EmojiType::Heart),
            "circle" => Some(EmojiType::Circle),
            "diamond" => Some(EmojiType::Diamond),
            "square" => Some(EmojiType::Square),
            "triangle" => Some(EmojiType::Triangle),
            "plus" => Some(EmojiType::Plus),
            "minus" => Some(EmojiType::Minus),
            "question" => Some(EmojiType::Question),
            "info" => Some(EmojiType::Info),
            // Emotions
            "thumbs_up" => Some(EmojiType::ThumbsUp),
            "thumbs_down" => Some(EmojiType::ThumbsDown),
            "fire" => Some(EmojiType::Fire),
            "rocket" => Some(EmojiType::Rocket),
            "skull" => Some(EmojiType::Skull),
            "crown" => Some(EmojiType::Crown),
            "gem" => Some(EmojiType::Gem),
            "poop" => Some(EmojiType::Poop),
            "frogger" => Some(EmojiType::Frogger),
            "frog" => Some(EmojiType::Frog),
            // Arrows
            "arrow_up" => Some(EmojiType::ArrowUp),
            "arrow_down" => Some(EmojiType::ArrowDown),
            "arrow_left" => Some(EmojiType::ArrowLeft),
            "arrow_right" => Some(EmojiType::ArrowRight),
            _ => None,
        }
    }

    /// Get string id for this emoji type
    pub fn id(&self) -> &'static str {
        match self {
            // Signals
            EmojiType::Target => "target",
            EmojiType::Flag => "flag",
            EmojiType::Check => "check",
            EmojiType::Cross => "cross",
            EmojiType::Warning => "warning",
            EmojiType::Dollar => "dollar",
            EmojiType::Lightning => "lightning",
            EmojiType::Lock => "lock",
            EmojiType::Unlock => "unlock",
            EmojiType::Bell => "bell",
            EmojiType::Eye => "eye",
            EmojiType::Clock => "clock",
            // Markers
            EmojiType::Star => "star",
            EmojiType::Heart => "heart",
            EmojiType::Circle => "circle",
            EmojiType::Diamond => "diamond",
            EmojiType::Square => "square",
            EmojiType::Triangle => "triangle",
            EmojiType::Plus => "plus",
            EmojiType::Minus => "minus",
            EmojiType::Question => "question",
            EmojiType::Info => "info",
            // Emotions
            EmojiType::ThumbsUp => "thumbs_up",
            EmojiType::ThumbsDown => "thumbs_down",
            EmojiType::Fire => "fire",
            EmojiType::Rocket => "rocket",
            EmojiType::Skull => "skull",
            EmojiType::Crown => "crown",
            EmojiType::Gem => "gem",
            EmojiType::Poop => "poop",
            EmojiType::Frogger => "frogger",
            EmojiType::Frog => "frog",
            // Arrows
            EmojiType::ArrowUp => "arrow_up",
            EmojiType::ArrowDown => "arrow_down",
            EmojiType::ArrowLeft => "arrow_left",
            EmojiType::ArrowRight => "arrow_right",
        }
    }
}

impl Default for EmojiType {
    fn default() -> Self {
        EmojiType::Target
    }
}

/// Emoji primitive with 5 data-coordinate anchor points
///
/// Points are stored as:
/// - center_bar, center_price: Center point
/// - radius_bars: Horizontal half-size in bars
/// - radius_price: Vertical half-size in price units
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Emoji {
    pub data: PrimitiveData,
    /// Center bar
    pub center_bar: f64,
    /// Center price
    pub center_price: f64,
    /// Horizontal radius in bars (distance from center to left/right edge)
    pub radius_bars: f64,
    /// Vertical radius in price units (distance from center to top/bottom edge)
    pub radius_price: f64,
    #[serde(default)]
    pub emoji_type: EmojiType,
}

fn default_radius_bars() -> f64 { 3.0 }
fn default_radius_price() -> f64 { 50.0 }

impl Emoji {
    pub fn new(bar: f64, price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "emoji".to_string(),
                display_name: "Emoji".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            center_bar: bar,
            center_price: price,
            radius_bars: default_radius_bars(),
            radius_price: default_radius_price(),
            emoji_type: EmojiType::Target,
        }
    }

    /// Create from center and edge point
    pub fn from_points(center_bar: f64, center_price: f64, edge_bar: f64, edge_price: f64, color: &str) -> Self {
        let radius_bars = (edge_bar - center_bar).abs().max(1.0);
        let radius_price = (edge_price - center_price).abs().max(1.0);
        let mut emoji = Self::new(center_bar, center_price, color);
        emoji.radius_bars = radius_bars;
        emoji.radius_price = radius_price;
        emoji
    }

    /// Render the emoji icon as vector graphics
    /// Size is calculated from screen coordinates - uses elliptical scaling
    fn render_icon(&self, ctx: &mut dyn RenderContext, cx: f64, cy: f64, half_w: f64, half_h: f64) {
        // Use separate horizontal (w) and vertical (h) scaling for elliptical rendering
        let w = half_w;
        let h = half_h;
        let color = &self.data.color.stroke;

        // Reset line dash to solid (may be set from previous render)
        ctx.set_line_dash(&[]);
        ctx.set_stroke_color(color);
        ctx.set_fill_color(color);
        ctx.set_stroke_width(2.0);
        ctx.set_line_cap("round");
        ctx.set_line_join("round");

        match self.emoji_type {
            // === SIGNALS ===
            EmojiType::Target => {
                // Target: concentric ellipses with crosshairs
                ctx.begin_path();
                ctx.ellipse(cx, cy, w * 0.9, h * 0.9, 0.0, 0.0, std::f64::consts::TAU);
                ctx.stroke();
                ctx.begin_path();
                ctx.ellipse(cx, cy, w * 0.5, h * 0.5, 0.0, 0.0, std::f64::consts::TAU);
                ctx.stroke();
                ctx.begin_path();
                ctx.ellipse(cx, cy, w * 0.15, h * 0.15, 0.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
                // Crosshairs
                ctx.begin_path();
                ctx.move_to(cx - w, cy);
                ctx.line_to(cx - w * 0.5, cy);
                ctx.move_to(cx + w * 0.5, cy);
                ctx.line_to(cx + w, cy);
                ctx.move_to(cx, cy - h);
                ctx.line_to(cx, cy - h * 0.5);
                ctx.move_to(cx, cy + h * 0.5);
                ctx.line_to(cx, cy + h);
                ctx.stroke();
            }
            EmojiType::Flag => {
                // Flag pole and banner
                ctx.begin_path();
                ctx.move_to(cx - w * 0.5, cy + h);
                ctx.line_to(cx - w * 0.5, cy - h);
                ctx.stroke();
                // Flag banner (triangle)
                ctx.begin_path();
                ctx.move_to(cx - w * 0.5, cy - h);
                ctx.line_to(cx + w * 0.7, cy - h * 0.5);
                ctx.line_to(cx - w * 0.5, cy);
                ctx.close_path();
                ctx.fill();
            }
            EmojiType::Check => {
                // Checkmark
                ctx.set_stroke_width(3.0);
                ctx.begin_path();
                ctx.move_to(cx - w * 0.6, cy);
                ctx.line_to(cx - w * 0.1, cy + h * 0.5);
                ctx.line_to(cx + w * 0.7, cy - h * 0.5);
                ctx.stroke();
            }
            EmojiType::Cross => {
                // X mark
                ctx.set_stroke_width(3.0);
                ctx.begin_path();
                ctx.move_to(cx - w * 0.5, cy - h * 0.5);
                ctx.line_to(cx + w * 0.5, cy + h * 0.5);
                ctx.move_to(cx + w * 0.5, cy - h * 0.5);
                ctx.line_to(cx - w * 0.5, cy + h * 0.5);
                ctx.stroke();
            }
            EmojiType::Warning => {
                // Triangle with exclamation
                ctx.begin_path();
                ctx.move_to(cx, cy - h * 0.8);
                ctx.line_to(cx + w * 0.8, cy + h * 0.6);
                ctx.line_to(cx - w * 0.8, cy + h * 0.6);
                ctx.close_path();
                ctx.stroke();
                // Exclamation
                ctx.begin_path();
                ctx.move_to(cx, cy - h * 0.3);
                ctx.line_to(cx, cy + h * 0.15);
                ctx.stroke();
                ctx.begin_path();
                ctx.ellipse(cx, cy + h * 0.35, w * 0.1, h * 0.1, 0.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
            }
            EmojiType::Dollar => {
                // Dollar sign
                ctx.set_stroke_width(2.5);
                ctx.begin_path();
                ctx.move_to(cx, cy - h * 0.9);
                ctx.line_to(cx, cy + h * 0.9);
                ctx.stroke();
                // S curve
                ctx.begin_path();
                ctx.move_to(cx + w * 0.5, cy - h * 0.5);
                ctx.bezier_curve_to(cx - w * 0.6, cy - h * 0.5, cx - w * 0.6, cy, cx, cy);
                ctx.bezier_curve_to(cx + w * 0.6, cy, cx + w * 0.6, cy + h * 0.5, cx - w * 0.5, cy + h * 0.5);
                ctx.stroke();
            }
            EmojiType::Lightning => {
                // Lightning bolt
                ctx.begin_path();
                ctx.move_to(cx + w * 0.1, cy - h * 0.9);
                ctx.line_to(cx - w * 0.5, cy);
                ctx.line_to(cx, cy);
                ctx.line_to(cx - w * 0.1, cy + h * 0.9);
                ctx.line_to(cx + w * 0.5, cy - h * 0.1);
                ctx.line_to(cx, cy - h * 0.1);
                ctx.close_path();
                ctx.fill();
            }
            EmojiType::Lock => {
                // Lock body
                ctx.begin_path();
                ctx.rect(cx - w * 0.5, cy, w, h * 0.7);
                ctx.fill();
                // Lock shackle
                ctx.set_stroke_width(2.5);
                ctx.begin_path();
                ctx.arc(cx, cy, w * 0.35, std::f64::consts::PI, 0.0);
                ctx.stroke();
            }
            EmojiType::Unlock => {
                // Lock body
                ctx.begin_path();
                ctx.rect(cx - w * 0.5, cy, w, h * 0.7);
                ctx.fill();
                // Open shackle
                ctx.set_stroke_width(2.5);
                ctx.begin_path();
                ctx.arc(cx, cy, w * 0.35, std::f64::consts::PI, std::f64::consts::PI * 0.2);
                ctx.stroke();
            }
            EmojiType::Bell => {
                // Bell body
                ctx.begin_path();
                ctx.move_to(cx - w * 0.6, cy + h * 0.4);
                ctx.quadratic_curve_to(cx - w * 0.6, cy - h * 0.6, cx, cy - h * 0.8);
                ctx.quadratic_curve_to(cx + w * 0.6, cy - h * 0.6, cx + w * 0.6, cy + h * 0.4);
                ctx.line_to(cx - w * 0.6, cy + h * 0.4);
                ctx.fill();
                // Bell bottom
                ctx.begin_path();
                ctx.ellipse(cx, cy + h * 0.4, w * 0.7, h * 0.15, 0.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
                // Clapper
                ctx.begin_path();
                ctx.ellipse(cx, cy + h * 0.7, w * 0.15, h * 0.15, 0.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
            }
            EmojiType::Eye => {
                // Eye outline
                ctx.begin_path();
                ctx.move_to(cx - w * 0.9, cy);
                ctx.quadratic_curve_to(cx, cy - h * 0.7, cx + w * 0.9, cy);
                ctx.quadratic_curve_to(cx, cy + h * 0.7, cx - w * 0.9, cy);
                ctx.stroke();
                // Iris
                ctx.begin_path();
                ctx.ellipse(cx, cy, w * 0.3, h * 0.35, 0.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
            }
            EmojiType::Clock => {
                // Clock face
                ctx.begin_path();
                ctx.ellipse(cx, cy, w * 0.8, h * 0.8, 0.0, 0.0, std::f64::consts::TAU);
                ctx.stroke();
                // Clock hands
                ctx.set_stroke_width(2.5);
                ctx.begin_path();
                ctx.move_to(cx, cy);
                ctx.line_to(cx, cy - h * 0.5);
                ctx.move_to(cx, cy);
                ctx.line_to(cx + w * 0.35, cy + h * 0.2);
                ctx.stroke();
            }

            // === MARKERS ===
            EmojiType::Star => {
                // 5-pointed star with elliptical scaling
                ctx.begin_path();
                for i in 0..10 {
                    let angle = std::f64::consts::PI / 2.0 + (i as f64) * std::f64::consts::PI / 5.0;
                    let r = if i % 2 == 0 { 1.0 } else { 0.4 };
                    let px = cx + w * r * angle.cos();
                    let py = cy - h * r * angle.sin();
                    if i == 0 { ctx.move_to(px, py); } else { ctx.line_to(px, py); }
                }
                ctx.close_path();
                ctx.fill();
            }
            EmojiType::Heart => {
                // Heart shape with elliptical scaling
                ctx.begin_path();
                ctx.move_to(cx, cy + h * 0.7);
                ctx.bezier_curve_to(cx - w * 0.8, cy + h * 0.2, cx - w * 0.8, cy - h * 0.5, cx, cy - h * 0.2);
                ctx.bezier_curve_to(cx + w * 0.8, cy - h * 0.5, cx + w * 0.8, cy + h * 0.2, cx, cy + h * 0.7);
                ctx.fill();
            }
            EmojiType::Circle => {
                // Filled ellipse
                ctx.begin_path();
                ctx.ellipse(cx, cy, w * 0.8, h * 0.8, 0.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
            }
            EmojiType::Diamond => {
                // Diamond shape
                ctx.begin_path();
                ctx.move_to(cx, cy - h * 0.9);
                ctx.line_to(cx + w * 0.7, cy);
                ctx.line_to(cx, cy + h * 0.9);
                ctx.line_to(cx - w * 0.7, cy);
                ctx.close_path();
                ctx.fill();
            }
            EmojiType::Square => {
                // Filled square
                ctx.begin_path();
                ctx.rect(cx - w * 0.7, cy - h * 0.7, w * 1.4, h * 1.4);
                ctx.fill();
            }
            EmojiType::Triangle => {
                // Filled triangle
                ctx.begin_path();
                ctx.move_to(cx, cy - h * 0.8);
                ctx.line_to(cx + w * 0.8, cy + h * 0.7);
                ctx.line_to(cx - w * 0.8, cy + h * 0.7);
                ctx.close_path();
                ctx.fill();
            }
            EmojiType::Plus => {
                // Plus sign
                ctx.set_stroke_width(3.0);
                ctx.begin_path();
                ctx.move_to(cx, cy - h * 0.7);
                ctx.line_to(cx, cy + h * 0.7);
                ctx.move_to(cx - w * 0.7, cy);
                ctx.line_to(cx + w * 0.7, cy);
                ctx.stroke();
            }
            EmojiType::Minus => {
                // Minus sign
                ctx.set_stroke_width(3.0);
                ctx.begin_path();
                ctx.move_to(cx - w * 0.7, cy);
                ctx.line_to(cx + w * 0.7, cy);
                ctx.stroke();
            }
            EmojiType::Question => {
                // Question mark in circle
                ctx.begin_path();
                ctx.ellipse(cx, cy, w * 0.8, h * 0.8, 0.0, 0.0, std::f64::consts::TAU);
                ctx.stroke();
                ctx.set_stroke_width(2.5);
                ctx.begin_path();
                ctx.arc(cx, cy - h * 0.25, w * 0.25, std::f64::consts::PI, 0.0);
                ctx.line_to(cx + w * 0.25, cy);
                ctx.quadratic_curve_to(cx + w * 0.25, cy + h * 0.15, cx, cy + h * 0.15);
                ctx.stroke();
                ctx.begin_path();
                ctx.ellipse(cx, cy + h * 0.4, w * 0.1, h * 0.1, 0.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
            }
            EmojiType::Info => {
                // Info icon in circle
                ctx.begin_path();
                ctx.ellipse(cx, cy, w * 0.8, h * 0.8, 0.0, 0.0, std::f64::consts::TAU);
                ctx.stroke();
                ctx.begin_path();
                ctx.ellipse(cx, cy - h * 0.4, w * 0.12, h * 0.12, 0.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.set_stroke_width(2.5);
                ctx.begin_path();
                ctx.move_to(cx, cy - h * 0.15);
                ctx.line_to(cx, cy + h * 0.5);
                ctx.stroke();
            }

            // === EMOTIONS ===
            EmojiType::ThumbsUp => {
                // Thumb
                ctx.begin_path();
                ctx.move_to(cx - w * 0.1, cy - h * 0.8);
                ctx.line_to(cx + w * 0.3, cy - h * 0.8);
                ctx.quadratic_curve_to(cx + w * 0.5, cy - h * 0.8, cx + w * 0.5, cy - h * 0.5);
                ctx.line_to(cx + w * 0.5, cy + h * 0.1);
                ctx.line_to(cx - w * 0.3, cy + h * 0.1);
                ctx.line_to(cx - w * 0.3, cy - h * 0.3);
                ctx.close_path();
                ctx.fill();
                // Fist
                ctx.begin_path();
                ctx.rect(cx - w * 0.6, cy + h * 0.1, w * 0.8, h * 0.6);
                ctx.fill();
            }
            EmojiType::ThumbsDown => {
                // Thumb down (inverted)
                ctx.begin_path();
                ctx.move_to(cx - w * 0.1, cy + h * 0.8);
                ctx.line_to(cx + w * 0.3, cy + h * 0.8);
                ctx.quadratic_curve_to(cx + w * 0.5, cy + h * 0.8, cx + w * 0.5, cy + h * 0.5);
                ctx.line_to(cx + w * 0.5, cy - h * 0.1);
                ctx.line_to(cx - w * 0.3, cy - h * 0.1);
                ctx.line_to(cx - w * 0.3, cy + h * 0.3);
                ctx.close_path();
                ctx.fill();
                // Fist
                ctx.begin_path();
                ctx.rect(cx - w * 0.6, cy - h * 0.7, w * 0.8, h * 0.6);
                ctx.fill();
            }
            EmojiType::Fire => {
                // Fire flame - outer orange/red
                ctx.set_fill_color("#FF6B35");
                ctx.begin_path();
                // Main flame shape
                ctx.move_to(cx, cy + h * 0.9);
                ctx.bezier_curve_to(cx - w * 0.7, cy + h * 0.6, cx - w * 0.8, cy - h * 0.1, cx - w * 0.4, cy - h * 0.5);
                ctx.bezier_curve_to(cx - w * 0.2, cy - h * 0.2, cx - w * 0.1, cy - h * 0.4, cx, cy - h * 0.9);
                ctx.bezier_curve_to(cx + w * 0.1, cy - h * 0.4, cx + w * 0.2, cy - h * 0.2, cx + w * 0.4, cy - h * 0.5);
                ctx.bezier_curve_to(cx + w * 0.8, cy - h * 0.1, cx + w * 0.7, cy + h * 0.6, cx, cy + h * 0.9);
                ctx.fill();
                // Inner yellow/orange core
                ctx.set_fill_color("#FFD93D");
                ctx.begin_path();
                ctx.move_to(cx, cy + h * 0.85);
                ctx.bezier_curve_to(cx - w * 0.4, cy + h * 0.5, cx - w * 0.35, cy + h * 0.1, cx - w * 0.15, cy - h * 0.2);
                ctx.bezier_curve_to(cx - w * 0.05, cy, cx, cy - h * 0.1, cx, cy - h * 0.5);
                ctx.bezier_curve_to(cx, cy - h * 0.1, cx + w * 0.05, cy, cx + w * 0.15, cy - h * 0.2);
                ctx.bezier_curve_to(cx + w * 0.35, cy + h * 0.1, cx + w * 0.4, cy + h * 0.5, cx, cy + h * 0.85);
                ctx.fill();
                // Reset color
                ctx.set_fill_color(color);
            }
            EmojiType::Rocket => {
                // Rocket body
                ctx.begin_path();
                ctx.move_to(cx, cy - h * 0.9);
                ctx.quadratic_curve_to(cx + w * 0.4, cy - h * 0.5, cx + w * 0.3, cy + h * 0.3);
                ctx.line_to(cx - w * 0.3, cy + h * 0.3);
                ctx.quadratic_curve_to(cx - w * 0.4, cy - h * 0.5, cx, cy - h * 0.9);
                ctx.fill();
                // Fins
                ctx.begin_path();
                ctx.move_to(cx - w * 0.3, cy + h * 0.1);
                ctx.line_to(cx - w * 0.6, cy + h * 0.5);
                ctx.line_to(cx - w * 0.3, cy + h * 0.3);
                ctx.fill();
                ctx.begin_path();
                ctx.move_to(cx + w * 0.3, cy + h * 0.1);
                ctx.line_to(cx + w * 0.6, cy + h * 0.5);
                ctx.line_to(cx + w * 0.3, cy + h * 0.3);
                ctx.fill();
                // Exhaust
                ctx.begin_path();
                ctx.move_to(cx - w * 0.2, cy + h * 0.3);
                ctx.line_to(cx, cy + h * 0.9);
                ctx.line_to(cx + w * 0.2, cy + h * 0.3);
                ctx.fill();
            }
            EmojiType::Skull => {
                // Skull head
                ctx.begin_path();
                ctx.ellipse(cx, cy - h * 0.15, w * 0.7, h * 0.6, 0.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
                // Eyes (hollow)
                ctx.set_fill_color("#000000");
                ctx.begin_path();
                ctx.ellipse(cx - w * 0.25, cy - h * 0.2, w * 0.15, h * 0.15, 0.0, 0.0, std::f64::consts::TAU);
                ctx.ellipse(cx + w * 0.25, cy - h * 0.2, w * 0.15, h * 0.15, 0.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
                // Jaw
                ctx.set_fill_color(color);
                ctx.begin_path();
                ctx.rect(cx - w * 0.4, cy + h * 0.3, w * 0.8, h * 0.4);
                ctx.fill();
                // Teeth lines
                ctx.set_stroke_color("#000000");
                ctx.set_stroke_width(1.5);
                ctx.begin_path();
                for i in 0..4 {
                    let tx = cx - w * 0.3 + (i as f64) * w * 0.2;
                    ctx.move_to(tx, cy + h * 0.3);
                    ctx.line_to(tx, cy + h * 0.7);
                }
                ctx.stroke();
                ctx.set_stroke_color(color);
            }
            EmojiType::Crown => {
                // Crown base
                ctx.begin_path();
                ctx.move_to(cx - w * 0.8, cy + h * 0.6);
                ctx.line_to(cx - w * 0.8, cy - h * 0.2);
                ctx.line_to(cx - w * 0.4, cy + h * 0.1);
                ctx.line_to(cx, cy - h * 0.7);
                ctx.line_to(cx + w * 0.4, cy + h * 0.1);
                ctx.line_to(cx + w * 0.8, cy - h * 0.2);
                ctx.line_to(cx + w * 0.8, cy + h * 0.6);
                ctx.close_path();
                ctx.fill();
            }
            EmojiType::Gem => {
                // Gem top facet
                ctx.begin_path();
                ctx.move_to(cx - w * 0.7, cy - h * 0.3);
                ctx.line_to(cx - w * 0.3, cy - h * 0.8);
                ctx.line_to(cx + w * 0.3, cy - h * 0.8);
                ctx.line_to(cx + w * 0.7, cy - h * 0.3);
                ctx.close_path();
                ctx.fill();
                // Gem bottom
                ctx.begin_path();
                ctx.move_to(cx - w * 0.7, cy - h * 0.3);
                ctx.line_to(cx + w * 0.7, cy - h * 0.3);
                ctx.line_to(cx, cy + h * 0.9);
                ctx.close_path();
                ctx.fill();
                // Facet lines
                ctx.set_stroke_width(1.5);
                ctx.begin_path();
                ctx.move_to(cx - w * 0.3, cy - h * 0.8);
                ctx.line_to(cx - w * 0.2, cy - h * 0.3);
                ctx.line_to(cx, cy + h * 0.9);
                ctx.move_to(cx + w * 0.3, cy - h * 0.8);
                ctx.line_to(cx + w * 0.2, cy - h * 0.3);
                ctx.line_to(cx, cy + h * 0.9);
                ctx.stroke();
            }
            EmojiType::Poop => {
                // Poop swirl
                ctx.begin_path();
                ctx.move_to(cx, cy - h * 0.8);
                ctx.bezier_curve_to(cx + w * 0.3, cy - h * 0.8, cx + w * 0.4, cy - h * 0.5, cx + w * 0.2, cy - h * 0.4);
                ctx.bezier_curve_to(cx + w * 0.5, cy - h * 0.3, cx + w * 0.6, cy, cx + w * 0.4, cy + h * 0.2);
                ctx.bezier_curve_to(cx + w * 0.7, cy + h * 0.3, cx + w * 0.8, cy + h * 0.7, cx, cy + h * 0.9);
                ctx.bezier_curve_to(cx - w * 0.8, cy + h * 0.7, cx - w * 0.7, cy + h * 0.3, cx - w * 0.4, cy + h * 0.2);
                ctx.bezier_curve_to(cx - w * 0.6, cy, cx - w * 0.5, cy - h * 0.3, cx - w * 0.2, cy - h * 0.4);
                ctx.bezier_curve_to(cx - w * 0.4, cy - h * 0.5, cx - w * 0.3, cy - h * 0.8, cx, cy - h * 0.8);
                ctx.fill();
                // Eyes
                ctx.set_fill_color("#FFFFFF");
                ctx.begin_path();
                ctx.ellipse(cx - w * 0.2, cy + h * 0.1, w * 0.1, h * 0.1, 0.0, 0.0, std::f64::consts::TAU);
                ctx.ellipse(cx + w * 0.2, cy + h * 0.1, w * 0.1, h * 0.1, 0.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.set_fill_color(color);
            }
            EmojiType::Frogger => {
                // Frogger - cute frog face (easter egg)
                // Main face (green ellipse)
                ctx.set_fill_color("#6B8E23");
                ctx.begin_path();
                ctx.ellipse(cx, cy + h * 0.1, w * 0.9, h * 0.7, 0.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
                // Eye bumps (green)
                ctx.begin_path();
                ctx.ellipse(cx - w * 0.4, cy - h * 0.3, w * 0.35, h * 0.3, 0.0, 0.0, std::f64::consts::TAU);
                ctx.ellipse(cx + w * 0.4, cy - h * 0.3, w * 0.35, h * 0.3, 0.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
                // Eye whites
                ctx.set_fill_color("#FFFFFF");
                ctx.begin_path();
                ctx.ellipse(cx - w * 0.4, cy - h * 0.3, w * 0.25, h * 0.2, 0.0, 0.0, std::f64::consts::TAU);
                ctx.ellipse(cx + w * 0.4, cy - h * 0.3, w * 0.25, h * 0.2, 0.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
                // Pupils
                ctx.set_fill_color("#000000");
                ctx.begin_path();
                ctx.ellipse(cx - w * 0.35, cy - h * 0.3, w * 0.1, h * 0.1, 0.0, 0.0, std::f64::consts::TAU);
                ctx.ellipse(cx + w * 0.35, cy - h * 0.3, w * 0.1, h * 0.1, 0.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
                // Sad/smug mouth
                ctx.set_stroke_color("#4a6b1a");
                ctx.set_stroke_width(2.0);
                ctx.begin_path();
                ctx.move_to(cx - w * 0.5, cy + h * 0.3);
                ctx.quadratic_curve_to(cx, cy + h * 0.6, cx + w * 0.5, cy + h * 0.3);
                ctx.stroke();
                // Blush cheeks
                ctx.set_fill_color("#ff9999");
                ctx.begin_path();
                ctx.ellipse(cx - w * 0.65, cy + h * 0.15, w * 0.15, h * 0.1, 0.0, 0.0, std::f64::consts::TAU);
                ctx.ellipse(cx + w * 0.65, cy + h * 0.15, w * 0.15, h * 0.1, 0.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
                // Reset color
                ctx.set_fill_color(color);
                ctx.set_stroke_color(color);
            }
            EmojiType::Frog => {
                // Frog with top hat (cylinder)
                // Main face shape (green, slightly wider at bottom)
                ctx.set_fill_color("#629632");
                ctx.begin_path();
                ctx.move_to(cx, cy - h * 0.5);
                ctx.bezier_curve_to(
                    cx + w * 0.6, cy - h * 0.5,
                    cx + w * 0.95, cy,
                    cx + w * 0.85, cy + h * 0.55,
                );
                ctx.bezier_curve_to(
                    cx + w * 0.75, cy + h * 0.95,
                    cx - w * 0.75, cy + h * 0.95,
                    cx - w * 0.85, cy + h * 0.55,
                );
                ctx.bezier_curve_to(
                    cx - w * 0.95, cy,
                    cx - w * 0.6, cy - h * 0.5,
                    cx, cy - h * 0.5,
                );
                ctx.close_path();
                ctx.fill();
                // Big bulging eyes (white)
                ctx.set_fill_color("#FFFFFF");
                ctx.begin_path();
                ctx.ellipse(
                    cx - w * 0.32,
                    cy - h * 0.05,
                    w * 0.38,
                    h * 0.38,
                    0.0,
                    0.0,
                    std::f64::consts::TAU,
                );
                ctx.ellipse(
                    cx + w * 0.32,
                    cy - h * 0.05,
                    w * 0.38,
                    h * 0.38,
                    0.0,
                    0.0,
                    std::f64::consts::TAU,
                );
                ctx.fill();
                // Green eyelids (droopy sad look)
                ctx.set_fill_color("#629632");
                ctx.begin_path();
                ctx.ellipse(
                    cx - w * 0.32,
                    cy - h * 0.25,
                    w * 0.4,
                    h * 0.22,
                    0.0,
                    0.0,
                    std::f64::consts::TAU,
                );
                ctx.ellipse(
                    cx + w * 0.32,
                    cy - h * 0.25,
                    w * 0.4,
                    h * 0.22,
                    0.0,
                    0.0,
                    std::f64::consts::TAU,
                );
                ctx.fill();
                // Pupils
                ctx.set_fill_color("#000000");
                ctx.begin_path();
                ctx.ellipse(
                    cx - w * 0.22,
                    cy + h * 0.02,
                    w * 0.14,
                    h * 0.18,
                    0.0,
                    0.0,
                    std::f64::consts::TAU,
                );
                ctx.ellipse(
                    cx + w * 0.22,
                    cy + h * 0.02,
                    w * 0.14,
                    h * 0.18,
                    0.0,
                    0.0,
                    std::f64::consts::TAU,
                );
                ctx.fill();
                // Wide frown/mouth
                ctx.set_fill_color("#4a7a2a");
                ctx.begin_path();
                ctx.move_to(cx - w * 0.6, cy + h * 0.5);
                ctx.quadratic_curve_to(cx, cy + h * 0.8, cx + w * 0.6, cy + h * 0.5);
                ctx.quadratic_curve_to(cx, cy + h * 0.65, cx - w * 0.6, cy + h * 0.5);
                ctx.close_path();
                ctx.fill();
                // TOP HAT (cylinder) - draw on top
                ctx.set_fill_color("#1a1a1a");
                // Hat brim (wide ellipse)
                ctx.begin_path();
                ctx.ellipse(cx, cy - h * 0.45, w * 0.7, h * 0.12, 0.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
                // Hat body (tall rectangle with rounded top)
                ctx.begin_path();
                ctx.rect(cx - w * 0.4, cy - h * 0.95, w * 0.8, h * 0.5);
                ctx.fill();
                // Hat top (ellipse)
                ctx.begin_path();
                ctx.ellipse(cx, cy - h * 0.95, w * 0.4, h * 0.1, 0.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
                // Reset color
                ctx.set_fill_color(color);
                ctx.set_stroke_color(color);
            }

            // === ARROWS ===
            EmojiType::ArrowUp => {
                // Up arrow
                ctx.begin_path();
                ctx.move_to(cx, cy - h * 0.8);
                ctx.line_to(cx - w * 0.5, cy);
                ctx.line_to(cx - w * 0.2, cy);
                ctx.line_to(cx - w * 0.2, cy + h * 0.8);
                ctx.line_to(cx + w * 0.2, cy + h * 0.8);
                ctx.line_to(cx + w * 0.2, cy);
                ctx.line_to(cx + w * 0.5, cy);
                ctx.close_path();
                ctx.fill();
            }
            EmojiType::ArrowDown => {
                // Down arrow
                ctx.begin_path();
                ctx.move_to(cx, cy + h * 0.8);
                ctx.line_to(cx - w * 0.5, cy);
                ctx.line_to(cx - w * 0.2, cy);
                ctx.line_to(cx - w * 0.2, cy - h * 0.8);
                ctx.line_to(cx + w * 0.2, cy - h * 0.8);
                ctx.line_to(cx + w * 0.2, cy);
                ctx.line_to(cx + w * 0.5, cy);
                ctx.close_path();
                ctx.fill();
            }
            EmojiType::ArrowLeft => {
                // Left arrow
                ctx.begin_path();
                ctx.move_to(cx - w * 0.8, cy);
                ctx.line_to(cx, cy - h * 0.5);
                ctx.line_to(cx, cy - h * 0.2);
                ctx.line_to(cx + w * 0.8, cy - h * 0.2);
                ctx.line_to(cx + w * 0.8, cy + h * 0.2);
                ctx.line_to(cx, cy + h * 0.2);
                ctx.line_to(cx, cy + h * 0.5);
                ctx.close_path();
                ctx.fill();
            }
            EmojiType::ArrowRight => {
                // Right arrow
                ctx.begin_path();
                ctx.move_to(cx + w * 0.8, cy);
                ctx.line_to(cx, cy - h * 0.5);
                ctx.line_to(cx, cy - h * 0.2);
                ctx.line_to(cx - w * 0.8, cy - h * 0.2);
                ctx.line_to(cx - w * 0.8, cy + h * 0.2);
                ctx.line_to(cx, cy + h * 0.2);
                ctx.line_to(cx, cy + h * 0.5);
                ctx.close_path();
                ctx.fill();
            }
        }
    }
}

impl Primitive for Emoji {
    fn type_id(&self) -> &'static str { "emoji" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Annotation }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    /// Returns 2 points: center and corner (for TwoPoint behavior)
    fn points(&self) -> Vec<(f64, f64)> {
        vec![
            (self.center_bar, self.center_price),
            (self.center_bar + self.radius_bars, self.center_price + self.radius_price),
        ]
    }

    fn set_points(&mut self, pts: &[(f64, f64)]) {
        if let Some(&(b, p)) = pts.first() {
            self.center_bar = b;
            self.center_price = p;
        }
        // Second point defines the corner (for TwoPoint creation)
        if let Some(&(b2, p2)) = pts.get(1) {
            self.radius_bars = (b2 - self.center_bar).abs().max(0.5);
            self.radius_price = (p2 - self.center_price).abs().max(1.0);
        }
    }

    fn translate(&mut self, bd: f64, pd: f64) {
        self.center_bar += bd;
        self.center_price += pd;
    }

    fn move_control_point(&mut self, pt: ControlPointType, bar: f64, price: f64) {
        match pt {
            ControlPointType::Move => {
                self.center_bar = bar;
                self.center_price = price;
            }
            ControlPointType::Edge(0) => {
                // Top - adjust vertical radius (price)
                self.radius_price = (price - self.center_price).abs().max(1.0);
            }
            ControlPointType::Edge(1) => {
                // Right - adjust horizontal radius (bars)
                self.radius_bars = (bar - self.center_bar).abs().max(0.5);
            }
            ControlPointType::Edge(2) => {
                // Bottom - adjust vertical radius
                self.radius_price = (self.center_price - price).abs().max(1.0);
            }
            ControlPointType::Edge(3) => {
                // Left - adjust horizontal radius
                self.radius_bars = (self.center_bar - bar).abs().max(0.5);
            }
            ControlPointType::Corner(_) => {
                // Proportional resize from corners
                self.radius_bars = (bar - self.center_bar).abs().max(0.5);
                self.radius_price = (price - self.center_price).abs().max(1.0);
            }
            _ => {}
        }
    }

    fn hit_test(&self, sx: f64, sy: f64, vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let cx = vp.bar_to_x_f64(self.center_bar);
        let cy = vp.price_to_y(self.center_price, ps.price_min, ps.price_max);

        // Calculate screen-space radii
        let rx = (vp.bar_to_x_f64(self.center_bar + self.radius_bars) - cx).abs();
        let ry = (vp.price_to_y(self.center_price + self.radius_price, ps.price_min, ps.price_max) - cy).abs();

        // Check corner control points first (for proportional resize)
        let corners = [
            (cx - rx, cy - ry, 0), // top-left
            (cx + rx, cy - ry, 1), // top-right
            (cx + rx, cy + ry, 2), // bottom-right
            (cx - rx, cy + ry, 3), // bottom-left
        ];
        for (corner_x, corner_y, idx) in corners {
            if ((sx - corner_x).powi(2) + (sy - corner_y).powi(2)).sqrt() < CONTROL_POINT_RADIUS as f64 + 4.0 {
                return HitTestResult::ControlPoint(ControlPointType::Corner(idx));
            }
        }

        // Check edge control points (cross pattern: top, right, bottom, left)
        let edges = [
            (cx, cy - ry, 0), // top
            (cx + rx, cy, 1), // right
            (cx, cy + ry, 2), // bottom
            (cx - rx, cy, 3), // left
        ];
        for (ex, ey, idx) in edges {
            if ((sx - ex).powi(2) + (sy - ey).powi(2)).sqrt() < CONTROL_POINT_RADIUS as f64 + 4.0 {
                return HitTestResult::ControlPoint(ControlPointType::Edge(idx));
            }
        }

        // Check center (move point)
        if ((sx - cx).powi(2) + (sy - cy).powi(2)).sqrt() < CONTROL_POINT_RADIUS as f64 + 4.0 {
            return HitTestResult::ControlPoint(ControlPointType::Move);
        }

        // Check body (bounding box)
        if sx >= cx - rx && sx <= cx + rx && sy >= cy - ry && sy <= cy + ry {
            return HitTestResult::Body;
        }

        HitTestResult::Miss
    }

    fn control_points(&self, vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let cx = vp.bar_to_x_f64(self.center_bar);
        let cy = vp.price_to_y(self.center_price, ps.price_min, ps.price_max);

        // Calculate screen-space radii
        let rx = (vp.bar_to_x_f64(self.center_bar + self.radius_bars) - cx).abs();
        let ry = (vp.price_to_y(self.center_price + self.radius_price, ps.price_min, ps.price_max) - cy).abs();

        vec![
            // Center move point
            ControlPoint::move_point(cx, cy),
            // Corner points (for proportional resize)
            ControlPoint::new(ControlPointType::Corner(0), cx - rx, cy - ry, ControlPointCursor::ResizeNWSE), // top-left
            ControlPoint::new(ControlPointType::Corner(1), cx + rx, cy - ry, ControlPointCursor::ResizeNESW), // top-right
            ControlPoint::new(ControlPointType::Corner(2), cx + rx, cy + ry, ControlPointCursor::ResizeNWSE), // bottom-right
            ControlPoint::new(ControlPointType::Corner(3), cx - rx, cy + ry, ControlPointCursor::ResizeNESW), // bottom-left
            // Edge points (cross pattern)
            ControlPoint::new(ControlPointType::Edge(0), cx, cy - ry, ControlPointCursor::ResizeNS),      // top
            ControlPoint::new(ControlPointType::Edge(1), cx + rx, cy, ControlPointCursor::ResizeEW),      // right
            ControlPoint::new(ControlPointType::Edge(2), cx, cy + ry, ControlPointCursor::ResizeNS),      // bottom
            ControlPoint::new(ControlPointType::Edge(3), cx - rx, cy, ControlPointCursor::ResizeEW),      // left
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let cx = ctx.bar_to_x(self.center_bar);
        let cy = ctx.price_to_y(self.center_price);

        // Calculate screen-space half-sizes from data coordinates
        let half_w = (ctx.bar_to_x(self.center_bar + self.radius_bars) - cx).abs();
        let half_h = (ctx.price_to_y(self.center_price + self.radius_price) - cy).abs();

        // Draw icon using vector graphics
        self.render_icon(ctx, cx, cy, half_w, half_h);

        // Draw control points if selected
        if is_selected {
            // Draw bounding box
            ctx.set_stroke_color("#2196F3");
            ctx.set_stroke_width(1.5);
            ctx.set_line_dash(&[4.0, 4.0]);
            ctx.begin_path();
            ctx.move_to(cx - half_w, cy - half_h);
            ctx.line_to(cx + half_w, cy - half_h);
            ctx.line_to(cx + half_w, cy + half_h);
            ctx.line_to(cx - half_w, cy + half_h);
            ctx.close_path();
            ctx.stroke();
            ctx.set_line_dash(&[]);

            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);

            // Draw corner control handles (for proportional resize)
            let corners = [
                (cx - half_w, cy - half_h), // top-left
                (cx + half_w, cy - half_h), // top-right
                (cx + half_w, cy + half_h), // bottom-right
                (cx - half_w, cy + half_h), // bottom-left
            ];
            for (corner_x, corner_y) in corners {
                ctx.begin_path();
                ctx.arc(corner_x, corner_y, CONTROL_POINT_RADIUS as f64, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }

            // Draw edge control handles (cross pattern)
            let edges = [
                (cx, cy - half_h), // top
                (cx + half_w, cy), // right
                (cx, cy + half_h), // bottom
                (cx - half_w, cy), // left
            ];
            for (ex, ey) in edges {
                ctx.begin_path();
                ctx.arc(ex, ey, CONTROL_POINT_RADIUS as f64, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }

            // Draw center move handle
            ctx.begin_path();
            ctx.arc(cx, cy, CONTROL_POINT_RADIUS as f64, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let min_x = cx - half_w;
                let max_x = cx + half_w;
                let min_y = cy - half_h;
                let max_y = cy + half_h;
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    TextAlign::Start => min_x,
                    TextAlign::Center => (min_x + max_x) / 2.0,
                    TextAlign::End => max_x,
                };
                // Start = ABOVE upper boundary, Center = middle, End = BELOW lower boundary
                let text_y = match text.v_align {
                    TextAlign::Start => min_y - text_offset,
                    TextAlign::Center => (min_y + max_y) / 2.0,
                    TextAlign::End => max_y + text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "emoji", display_name: "Sticker", kind: PrimitiveKind::Annotation,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Icon marker", icon: "emoji", default_color: "#FFC107",
        factory: |points, color| {
            let (b1, p1) = points.first().copied().unwrap_or((0.0, 100.0));
            let (b2, p2) = points.get(1).copied().unwrap_or((b1 + 5.0, p1 + 50.0));
            Box::new(Emoji::from_points(b1, p1, b2, p2, color))
        },
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}

/// Generate metadata for a specific emoji type as a standalone primitive
fn emoji_metadata(emoji_type: EmojiType) -> PrimitiveMetadata {
    let type_id: &'static str = match emoji_type {
        EmojiType::Target => "emoji_target",
        EmojiType::Flag => "emoji_flag",
        EmojiType::Check => "emoji_check",
        EmojiType::Cross => "emoji_cross",
        EmojiType::Warning => "emoji_warning",
        EmojiType::Dollar => "emoji_dollar",
        EmojiType::Lightning => "emoji_lightning",
        EmojiType::Lock => "emoji_lock",
        EmojiType::Unlock => "emoji_unlock",
        EmojiType::Bell => "emoji_bell",
        EmojiType::Eye => "emoji_eye",
        EmojiType::Clock => "emoji_clock",
        EmojiType::Star => "emoji_star",
        EmojiType::Heart => "emoji_heart",
        EmojiType::Circle => "emoji_circle",
        EmojiType::Diamond => "emoji_diamond",
        EmojiType::Square => "emoji_square",
        EmojiType::Triangle => "emoji_triangle",
        EmojiType::Plus => "emoji_plus",
        EmojiType::Minus => "emoji_minus",
        EmojiType::Question => "emoji_question",
        EmojiType::Info => "emoji_info",
        EmojiType::ThumbsUp => "emoji_thumbs_up",
        EmojiType::ThumbsDown => "emoji_thumbs_down",
        EmojiType::Fire => "emoji_fire",
        EmojiType::Rocket => "emoji_rocket",
        EmojiType::Skull => "emoji_skull",
        EmojiType::Crown => "emoji_crown",
        EmojiType::Gem => "emoji_gem",
        EmojiType::Poop => "emoji_poop",
        EmojiType::Frogger => "emoji_frogger",
        EmojiType::Frog => "emoji_frog",
        EmojiType::ArrowUp => "emoji_arrow_up",
        EmojiType::ArrowDown => "emoji_arrow_down",
        EmojiType::ArrowLeft => "emoji_arrow_left",
        EmojiType::ArrowRight => "emoji_arrow_right",
    };

    let display_name = emoji_type.label();
    let icon = emoji_type.id();

    PrimitiveMetadata {
        type_id,
        display_name,
        kind: PrimitiveKind::Annotation,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: display_name,
        icon,
        default_color: "#FFC107",
        factory: create_emoji_factory(emoji_type),
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}

/// Create factory function for specific emoji type
fn create_emoji_factory(emoji_type: EmojiType) -> fn(&[(f64, f64)], &str) -> Box<dyn Primitive> {
    match emoji_type {
        EmojiType::Target => |points, color| create_emoji_primitive(EmojiType::Target, points, color),
        EmojiType::Flag => |points, color| create_emoji_primitive(EmojiType::Flag, points, color),
        EmojiType::Check => |points, color| create_emoji_primitive(EmojiType::Check, points, color),
        EmojiType::Cross => |points, color| create_emoji_primitive(EmojiType::Cross, points, color),
        EmojiType::Warning => |points, color| create_emoji_primitive(EmojiType::Warning, points, color),
        EmojiType::Dollar => |points, color| create_emoji_primitive(EmojiType::Dollar, points, color),
        EmojiType::Lightning => |points, color| create_emoji_primitive(EmojiType::Lightning, points, color),
        EmojiType::Lock => |points, color| create_emoji_primitive(EmojiType::Lock, points, color),
        EmojiType::Unlock => |points, color| create_emoji_primitive(EmojiType::Unlock, points, color),
        EmojiType::Bell => |points, color| create_emoji_primitive(EmojiType::Bell, points, color),
        EmojiType::Eye => |points, color| create_emoji_primitive(EmojiType::Eye, points, color),
        EmojiType::Clock => |points, color| create_emoji_primitive(EmojiType::Clock, points, color),
        EmojiType::Star => |points, color| create_emoji_primitive(EmojiType::Star, points, color),
        EmojiType::Heart => |points, color| create_emoji_primitive(EmojiType::Heart, points, color),
        EmojiType::Circle => |points, color| create_emoji_primitive(EmojiType::Circle, points, color),
        EmojiType::Diamond => |points, color| create_emoji_primitive(EmojiType::Diamond, points, color),
        EmojiType::Square => |points, color| create_emoji_primitive(EmojiType::Square, points, color),
        EmojiType::Triangle => |points, color| create_emoji_primitive(EmojiType::Triangle, points, color),
        EmojiType::Plus => |points, color| create_emoji_primitive(EmojiType::Plus, points, color),
        EmojiType::Minus => |points, color| create_emoji_primitive(EmojiType::Minus, points, color),
        EmojiType::Question => |points, color| create_emoji_primitive(EmojiType::Question, points, color),
        EmojiType::Info => |points, color| create_emoji_primitive(EmojiType::Info, points, color),
        EmojiType::ThumbsUp => |points, color| create_emoji_primitive(EmojiType::ThumbsUp, points, color),
        EmojiType::ThumbsDown => |points, color| create_emoji_primitive(EmojiType::ThumbsDown, points, color),
        EmojiType::Fire => |points, color| create_emoji_primitive(EmojiType::Fire, points, color),
        EmojiType::Rocket => |points, color| create_emoji_primitive(EmojiType::Rocket, points, color),
        EmojiType::Skull => |points, color| create_emoji_primitive(EmojiType::Skull, points, color),
        EmojiType::Crown => |points, color| create_emoji_primitive(EmojiType::Crown, points, color),
        EmojiType::Gem => |points, color| create_emoji_primitive(EmojiType::Gem, points, color),
        EmojiType::Poop => |points, color| create_emoji_primitive(EmojiType::Poop, points, color),
        EmojiType::Frogger => |points, color| create_emoji_primitive(EmojiType::Frogger, points, color),
        EmojiType::Frog => |points, color| create_emoji_primitive(EmojiType::Frog, points, color),
        EmojiType::ArrowUp => |points, color| create_emoji_primitive(EmojiType::ArrowUp, points, color),
        EmojiType::ArrowDown => |points, color| create_emoji_primitive(EmojiType::ArrowDown, points, color),
        EmojiType::ArrowLeft => |points, color| create_emoji_primitive(EmojiType::ArrowLeft, points, color),
        EmojiType::ArrowRight => |points, color| create_emoji_primitive(EmojiType::ArrowRight, points, color),
    }
}

fn create_emoji_primitive(emoji_type: EmojiType, points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (b1, p1) = points.first().copied().unwrap_or((0.0, 100.0));
    let (b2, p2) = points.get(1).copied().unwrap_or((b1 + 5.0, p1 + 50.0));
    let mut emoji = Emoji::from_points(b1, p1, b2, p2, color);
    emoji.emoji_type = emoji_type;
    Box::new(emoji)
}

/// Get all emoji metadata for registration
pub fn all_emoji_metadata() -> Vec<PrimitiveMetadata> {
    EmojiType::all().iter().map(|t| emoji_metadata(t.clone())).collect()
}
