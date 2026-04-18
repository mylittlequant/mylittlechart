use crate::{TimedSnapshot, OrderbookStoreError};
use std::path::Path;

const MAGIC: u8 = 0xBC;
const VERSION: u8 = 0x01;
// Header: magic(1) + version(1) + snap_count(4) + crc(4) = 10 bytes
const HEADER_SIZE: usize = 10;
const LEVEL_SIZE: usize = 16; // (f64 price, f64 qty) = 8 + 8

/// Serialize all snapshots into a byte payload.
fn serialize_snapshots(snapshots: &[TimedSnapshot]) -> Vec<u8> {
    let mut buf = Vec::new();
    for snap in snapshots {
        buf.extend_from_slice(&snap.timestamp_ms.to_le_bytes());
        buf.extend_from_slice(&(snap.bids.len() as u32).to_le_bytes());
        buf.extend_from_slice(&(snap.asks.len() as u32).to_le_bytes());
        for &(price, qty) in &snap.bids {
            buf.extend_from_slice(&price.to_le_bytes());
            buf.extend_from_slice(&qty.to_le_bytes());
        }
        for &(price, qty) in &snap.asks {
            buf.extend_from_slice(&price.to_le_bytes());
            buf.extend_from_slice(&qty.to_le_bytes());
        }
    }
    buf
}

/// Deserialize snapshots from payload bytes.
fn deserialize_snapshots(payload: &[u8], snap_count: usize) -> Result<Vec<TimedSnapshot>, OrderbookStoreError> {
    let mut snapshots = Vec::with_capacity(snap_count);
    let mut pos = 0;

    for _ in 0..snap_count {
        if pos + 16 > payload.len() {
            return Err(OrderbookStoreError::Truncated { expected: snap_count });
        }
        let timestamp_ms = i64::from_le_bytes(payload[pos..pos + 8].try_into().unwrap());
        pos += 8;
        let bids_count = u32::from_le_bytes(payload[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        let asks_count = u32::from_le_bytes(payload[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;

        let bids_bytes = bids_count * LEVEL_SIZE;
        let asks_bytes = asks_count * LEVEL_SIZE;

        if pos + bids_bytes + asks_bytes > payload.len() {
            return Err(OrderbookStoreError::Truncated { expected: snap_count });
        }

        let mut bids = Vec::with_capacity(bids_count);
        for _ in 0..bids_count {
            let price = f64::from_le_bytes(payload[pos..pos + 8].try_into().unwrap());
            pos += 8;
            let qty = f64::from_le_bytes(payload[pos..pos + 8].try_into().unwrap());
            pos += 8;
            bids.push((price, qty));
        }

        let mut asks = Vec::with_capacity(asks_count);
        for _ in 0..asks_count {
            let price = f64::from_le_bytes(payload[pos..pos + 8].try_into().unwrap());
            pos += 8;
            let qty = f64::from_le_bytes(payload[pos..pos + 8].try_into().unwrap());
            pos += 8;
            asks.push((price, qty));
        }

        snapshots.push(TimedSnapshot { timestamp_ms, bids, asks });
    }

    Ok(snapshots)
}

/// Write a list of `TimedSnapshot`s to a file atomically (write .tmp then rename).
pub fn write_snapshots(path: &Path, snapshots: &[TimedSnapshot]) -> Result<(), OrderbookStoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| OrderbookStoreError::Io { path: parent.to_path_buf(), source: e })?;
    }

    let snap_count = snapshots.len() as u32;
    let payload = serialize_snapshots(snapshots);
    let crc = crc32fast::hash(&payload);

    let mut buf = Vec::with_capacity(HEADER_SIZE + payload.len());
    buf.push(MAGIC);
    buf.push(VERSION);
    buf.extend_from_slice(&snap_count.to_le_bytes());
    buf.extend_from_slice(&crc.to_le_bytes());
    buf.extend_from_slice(&payload);

    // Atomic write: .tmp then rename
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &buf)
        .map_err(|e| OrderbookStoreError::Io { path: tmp.clone(), source: e })?;
    std::fs::rename(&tmp, path)
        .map_err(|e| OrderbookStoreError::Io { path: path.to_path_buf(), source: e })?;

    Ok(())
}

/// Read a list of `TimedSnapshot`s from a file, validating header and CRC.
pub fn read_snapshots(path: &Path) -> Result<Vec<TimedSnapshot>, OrderbookStoreError> {
    let data = std::fs::read(path)
        .map_err(|e| OrderbookStoreError::Io { path: path.to_path_buf(), source: e })?;

    if data.len() < HEADER_SIZE {
        return Err(OrderbookStoreError::Truncated { expected: 0 });
    }
    if data[0] != MAGIC {
        return Err(OrderbookStoreError::BadMagic(data[0]));
    }
    if data[1] != VERSION {
        return Err(OrderbookStoreError::UnsupportedVersion(data[1]));
    }

    let snap_count = u32::from_le_bytes(data[2..6].try_into().unwrap()) as usize;
    let stored_crc = u32::from_le_bytes(data[6..10].try_into().unwrap());
    let payload = &data[HEADER_SIZE..];

    let computed_crc = crc32fast::hash(payload);
    if computed_crc != stored_crc {
        return Err(OrderbookStoreError::CrcMismatch { stored: stored_crc, computed: computed_crc });
    }

    deserialize_snapshots(payload, snap_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_snapshot(ts: i64, n_levels: usize) -> TimedSnapshot {
        let bids = (0..n_levels).map(|i| (30000.0 - i as f64, 1.0 + i as f64 * 0.1)).collect();
        let asks = (0..n_levels).map(|i| (30001.0 + i as f64, 1.0 + i as f64 * 0.1)).collect();
        TimedSnapshot { timestamp_ms: ts, bids, asks }
    }

    #[test]
    fn round_trip_single() {
        let dir = std::env::temp_dir().join("ob_store_test_single");
        let path = dir.join("test.bin");
        let snap = sample_snapshot(1_700_000_000_000, 10);
        write_snapshots(&path, &[snap.clone()]).unwrap();
        let loaded = read_snapshots(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].timestamp_ms, snap.timestamp_ms);
        assert_eq!(loaded[0].bids.len(), snap.bids.len());
        assert_eq!(loaded[0].asks.len(), snap.asks.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn round_trip_multiple() {
        let dir = std::env::temp_dir().join("ob_store_test_multiple");
        let path = dir.join("test.bin");
        let snaps: Vec<_> = (0..5).map(|i| sample_snapshot(i as i64 * 1000, 5)).collect();
        write_snapshots(&path, &snaps).unwrap();
        let loaded = read_snapshots(&path).unwrap();
        assert_eq!(loaded.len(), 5);
        for (a, b) in snaps.iter().zip(loaded.iter()) {
            assert_eq!(a.timestamp_ms, b.timestamp_ms);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_snapshots() {
        let dir = std::env::temp_dir().join("ob_store_test_empty");
        let path = dir.join("empty.bin");
        write_snapshots(&path, &[]).unwrap();
        let loaded = read_snapshots(&path).unwrap();
        assert!(loaded.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_magic() {
        let dir = std::env::temp_dir().join("ob_store_test_magic");
        let path = dir.join("bad.bin");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, &[0x00u8, 0x01, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap();
        assert!(matches!(read_snapshots(&path), Err(OrderbookStoreError::BadMagic(0x00))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn crc_corruption() {
        let dir = std::env::temp_dir().join("ob_store_test_crc");
        let path = dir.join("corrupt.bin");
        let snap = sample_snapshot(1_000, 3);
        write_snapshots(&path, &[snap]).unwrap();
        let mut data = std::fs::read(&path).unwrap();
        if data.len() > HEADER_SIZE {
            data[HEADER_SIZE] ^= 0xFF;
        }
        std::fs::write(&path, &data).unwrap();
        assert!(matches!(read_snapshots(&path), Err(OrderbookStoreError::CrcMismatch { .. })));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
