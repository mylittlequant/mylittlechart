use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum BarStoreError {
    #[error("I/O error for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid magic byte 0x{0:02X} (expected 0xBA)")]
    BadMagic(u8),

    #[error("unsupported version {0}")]
    UnsupportedVersion(u8),

    #[error("CRC mismatch: stored {stored:#010x}, computed {computed:#010x}")]
    CrcMismatch { stored: u32, computed: u32 },

    #[error("truncated file: expected {expected} bars but payload too short")]
    Truncated { expected: usize },
}
