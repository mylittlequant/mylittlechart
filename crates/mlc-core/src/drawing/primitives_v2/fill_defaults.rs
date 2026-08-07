//! Default fill opacity for drawing primitives.
//!
//! Before this module, `fn default_fill_opacity() -> f64` was copy-pasted as a
//! private helper into nineteen primitive files with four different values
//! (0.1, 0.12, 0.2, 0.88, 0.9). Nothing tied them together, so each new
//! primitive picked whichever number its author happened to look at, and the
//! annotation plates drifted to effectively opaque.
//!
//! Owner rule (2026-08-07): a default object must never ship an opaque fill —
//! it hides the candles it is drawn over, which is the one thing a trading
//! chart cannot afford. Full opacity stays reachable, but as a deliberate
//! choice in the primitive's settings, never as the default.
//!
//! Two constants, because one number is wrong for both cases:
//!
//! * [`AREA_FILL_OPACITY`] — regions that cover chart area (shapes, channels,
//!   zones, Fibonacci/Gann level bands). These must be barely-there; the
//!   candles under them are the point.
//! * [`PLATE_FILL_OPACITY`] — the backing plate of a text annotation. Its job
//!   is to make text legible, so it cannot go as low as an area fill, but it
//!   is still translucent enough to read the chart through.

/// Fill opacity for any region drawn over chart area.
///
/// Matches what `shapes/*` and `channels/*` already used, so this constant
/// changes nothing for them — it just stops the value from being re-typed.
pub const AREA_FILL_OPACITY: f64 = 0.12;

/// Fill opacity for a level band (Fibonacci, Gann, pitchfork).
///
/// Slightly lower than [`AREA_FILL_OPACITY`] because these stack: a
/// retracement draws up to eleven bands at once and they overlap at the edges.
pub const BAND_FILL_OPACITY: f64 = 0.1;

/// Fill opacity for the backing plate of a SMALL text annotation — a chip
/// carrying a word or a price, drawn at the size of its own text.
///
/// Replaces the 0.88/0.9 plates, which read as solid boxes punched into the
/// chart. Text stays legible at this value against both themes; the candles
/// behind it stay visible, which is the whole complaint. It covers so little
/// area that the trade-off lands on the side of legibility.
pub const PLATE_FILL_OPACITY: f64 = 0.55;

/// Fill opacity for a text annotation the USER SIZES — `note`, `table`.
///
/// These are not chips. A note dragged across two days of candles is an area
/// fill that happens to carry text, and at [`PLATE_FILL_OPACITY`] it reads as
/// a wall: measured on the live stand, a default note washed out every candle
/// behind it. What makes such an annotation readable is its outline and its
/// text, not the strength of its backing — so it sits with the area fills,
/// just high enough to still register as a surface.
///
/// Full opacity stays one settings row away for anyone who wants a sticky
/// note that hides what is under it.
pub const BOX_ANNOTATION_FILL_OPACITY: f64 = 0.2;

/// `serde(default)` shim for [`AREA_FILL_OPACITY`].
pub fn default_area_fill_opacity() -> f64 { AREA_FILL_OPACITY }

/// `serde(default)` shim for [`BAND_FILL_OPACITY`].
pub fn default_band_fill_opacity() -> f64 { BAND_FILL_OPACITY }

/// `serde(default)` shim for [`PLATE_FILL_OPACITY`].
pub fn default_plate_fill_opacity() -> f64 { PLATE_FILL_OPACITY }

/// `serde(default)` shim for [`BOX_ANNOTATION_FILL_OPACITY`].
pub fn default_box_annotation_fill_opacity() -> f64 { BOX_ANNOTATION_FILL_OPACITY }
