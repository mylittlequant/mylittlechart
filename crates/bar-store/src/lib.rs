mod error;
mod format;
mod store;

pub use error::BarStoreError;
pub use store::BarStoreHandle;

/// OHLCV bar — must match the layout of `zengeld_chart::Bar` exactly.
///
/// 48 bytes with `#[repr(C)]`: `i64` + 5 × `f64`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct Bar {
    pub timestamp: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}
