use crate::{Bar, BarStoreError};
use std::path::Path;

const MAGIC: u8 = 0xBA;
const VERSION: u8 = 0x01;
const HEADER_SIZE: usize = 10;
const BAR_SIZE: usize = 48; // size_of::<Bar>() with repr(C)

const _: () = assert!(std::mem::size_of::<Bar>() == BAR_SIZE);

/// Serialize a single `Bar` into 48 bytes (little-endian fields).
fn bar_to_bytes(bar: &Bar) -> [u8; BAR_SIZE] {
    let mut b = [0u8; BAR_SIZE];
    b[0..8].copy_from_slice(&bar.timestamp.to_le_bytes());
    b[8..16].copy_from_slice(&bar.open.to_le_bytes());
    b[16..24].copy_from_slice(&bar.high.to_le_bytes());
    b[24..32].copy_from_slice(&bar.low.to_le_bytes());
    b[32..40].copy_from_slice(&bar.close.to_le_bytes());
    b[40..48].copy_from_slice(&bar.volume.to_le_bytes());
    b
}

/// Deserialize a single `Bar` from 48 bytes (little-endian fields).
fn bar_from_bytes(b: &[u8; BAR_SIZE]) -> Bar {
    Bar {
        timestamp: i64::from_le_bytes(b[0..8].try_into().unwrap()),
        open: f64::from_le_bytes(b[8..16].try_into().unwrap()),
        high: f64::from_le_bytes(b[16..24].try_into().unwrap()),
        low: f64::from_le_bytes(b[24..32].try_into().unwrap()),
        close: f64::from_le_bytes(b[32..40].try_into().unwrap()),
        volume: f64::from_le_bytes(b[40..48].try_into().unwrap()),
    }
}

/// Write bars to a file atomically (write .tmp then rename).
pub fn write_bars(path: &Path, bars: &[Bar]) -> Result<(), BarStoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| BarStoreError::Io { path: parent.to_path_buf(), source: e })?;
    }

    let bar_count = bars.len() as u32;
    let payload_len = bars.len() * BAR_SIZE;

    // Serialize bars into a flat byte payload
    let mut payload = Vec::with_capacity(payload_len);
    for bar in bars {
        payload.extend_from_slice(&bar_to_bytes(bar));
    }

    let crc = crc32fast::hash(&payload);

    let mut buf = Vec::with_capacity(HEADER_SIZE + payload_len);
    buf.push(MAGIC);
    buf.push(VERSION);
    buf.extend_from_slice(&bar_count.to_le_bytes());
    buf.extend_from_slice(&crc.to_le_bytes());
    buf.extend_from_slice(&payload);

    // Atomic write: .tmp then rename
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &buf)
        .map_err(|e| BarStoreError::Io { path: tmp.clone(), source: e })?;
    std::fs::rename(&tmp, path)
        .map_err(|e| BarStoreError::Io { path: path.to_path_buf(), source: e })?;

    Ok(())
}

/// Read bars from a file, validating header and CRC.
pub fn read_bars(path: &Path) -> Result<Vec<Bar>, BarStoreError> {
    let data = std::fs::read(path)
        .map_err(|e| BarStoreError::Io { path: path.to_path_buf(), source: e })?;

    if data.len() < HEADER_SIZE {
        return Err(BarStoreError::Truncated { expected: 0 });
    }
    if data[0] != MAGIC {
        return Err(BarStoreError::BadMagic(data[0]));
    }
    if data[1] != VERSION {
        return Err(BarStoreError::UnsupportedVersion(data[1]));
    }

    let bar_count = u32::from_le_bytes(data[2..6].try_into().unwrap()) as usize;
    let stored_crc = u32::from_le_bytes(data[6..10].try_into().unwrap());
    let payload = &data[HEADER_SIZE..];
    let expected_len = bar_count * BAR_SIZE;

    if payload.len() < expected_len {
        return Err(BarStoreError::Truncated { expected: bar_count });
    }

    let computed_crc = crc32fast::hash(&payload[..expected_len]);
    if computed_crc != stored_crc {
        return Err(BarStoreError::CrcMismatch { stored: stored_crc, computed: computed_crc });
    }

    // Deserialize field-by-field — avoids any alignment assumptions on the raw bytes
    let mut bars = Vec::with_capacity(bar_count);
    for i in 0..bar_count {
        let offset = i * BAR_SIZE;
        let chunk: &[u8; BAR_SIZE] = payload[offset..offset + BAR_SIZE].try_into().unwrap();
        bars.push(bar_from_bytes(chunk));
    }

    Ok(bars)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Bar;

    fn sample_bars(n: usize) -> Vec<Bar> {
        (0..n)
            .map(|i| Bar {
                timestamp: 1700000000 + (i as i64 * 3600),
                open: 100.0 + i as f64,
                high: 110.0 + i as f64,
                low: 90.0 + i as f64,
                close: 105.0 + i as f64,
                volume: 1000.0 * (i + 1) as f64,
            })
            .collect()
    }

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join("bar_store_test_roundtrip");
        let path = dir.join("test.bin");
        let bars = sample_bars(100);
        write_bars(&path, &bars).unwrap();
        let loaded = read_bars(&path).unwrap();
        assert_eq!(bars, loaded);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_bars() {
        let dir = std::env::temp_dir().join("bar_store_test_empty");
        let path = dir.join("empty.bin");
        write_bars(&path, &[]).unwrap();
        let loaded = read_bars(&path).unwrap();
        assert!(loaded.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_magic() {
        let dir = std::env::temp_dir().join("bar_store_test_magic");
        let path = dir.join("bad.bin");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, &[0x00u8, 0x01, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap();
        assert!(matches!(read_bars(&path), Err(BarStoreError::BadMagic(0x00))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn crc_corruption() {
        let dir = std::env::temp_dir().join("bar_store_test_crc");
        let path = dir.join("corrupt.bin");
        let bars = sample_bars(10);
        write_bars(&path, &bars).unwrap();
        // Flip a byte in the payload
        let mut data = std::fs::read(&path).unwrap();
        data[HEADER_SIZE] ^= 0xFF;
        std::fs::write(&path, &data).unwrap();
        assert!(matches!(read_bars(&path), Err(BarStoreError::CrcMismatch { .. })));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn truncated() {
        let dir = std::env::temp_dir().join("bar_store_test_trunc");
        let path = dir.join("trunc.bin");
        std::fs::create_dir_all(&dir).unwrap();
        // Valid header claiming 100 bars, but empty payload
        let mut buf = vec![MAGIC, VERSION];
        buf.extend_from_slice(&100u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes()); // CRC doesn't matter, truncated check comes first
        std::fs::write(&path, &buf).unwrap();
        assert!(matches!(read_bars(&path), Err(BarStoreError::Truncated { expected: 100 })));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bar_size_assertion() {
        assert_eq!(std::mem::size_of::<Bar>(), BAR_SIZE);
    }
}
