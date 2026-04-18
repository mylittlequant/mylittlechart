use crate::{Trade, TradeStoreError};
use std::path::Path;

const MAGIC: u8 = 0xBB;
const VERSION: u8 = 0x01;
const HEADER_SIZE: usize = 10;
const TRADE_SIZE: usize = 40; // size_of::<Trade>() with repr(C)

const _: () = assert!(std::mem::size_of::<Trade>() == TRADE_SIZE);

/// Serialize a single `Trade` into 40 bytes (little-endian fields).
fn trade_to_bytes(trade: &Trade) -> [u8; TRADE_SIZE] {
    let mut b = [0u8; TRADE_SIZE];
    b[0..8].copy_from_slice(&trade.timestamp_ms.to_le_bytes());
    b[8..16].copy_from_slice(&trade.price.to_le_bytes());
    b[16..24].copy_from_slice(&trade.quantity.to_le_bytes());
    b[24..32].copy_from_slice(&trade.trade_id.to_le_bytes());
    b[32] = trade.is_buyer_maker;
    // b[33..40] = padding zeros (already zero from array init)
    b
}

/// Deserialize a single `Trade` from 40 bytes (little-endian fields).
fn trade_from_bytes(b: &[u8; TRADE_SIZE]) -> Trade {
    Trade {
        timestamp_ms: i64::from_le_bytes(b[0..8].try_into().unwrap()),
        price: f64::from_le_bytes(b[8..16].try_into().unwrap()),
        quantity: f64::from_le_bytes(b[16..24].try_into().unwrap()),
        trade_id: u64::from_le_bytes(b[24..32].try_into().unwrap()),
        is_buyer_maker: b[32],
        _pad: [0u8; 7],
    }
}

/// Write trades to a file atomically (write .tmp then rename).
pub fn write_trades(path: &Path, trades: &[Trade]) -> Result<(), TradeStoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| TradeStoreError::Io { path: parent.to_path_buf(), source: e })?;
    }

    let trade_count = trades.len() as u32;
    let payload_len = trades.len() * TRADE_SIZE;

    // Serialize trades into a flat byte payload
    let mut payload = Vec::with_capacity(payload_len);
    for trade in trades {
        payload.extend_from_slice(&trade_to_bytes(trade));
    }

    let crc = crc32fast::hash(&payload);

    let mut buf = Vec::with_capacity(HEADER_SIZE + payload_len);
    buf.push(MAGIC);
    buf.push(VERSION);
    buf.extend_from_slice(&trade_count.to_le_bytes());
    buf.extend_from_slice(&crc.to_le_bytes());
    buf.extend_from_slice(&payload);

    // Atomic write: .tmp then rename
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &buf)
        .map_err(|e| TradeStoreError::Io { path: tmp.clone(), source: e })?;
    std::fs::rename(&tmp, path)
        .map_err(|e| TradeStoreError::Io { path: path.to_path_buf(), source: e })?;

    Ok(())
}

/// Read trades from a file, validating header and CRC.
pub fn read_trades(path: &Path) -> Result<Vec<Trade>, TradeStoreError> {
    let data = std::fs::read(path)
        .map_err(|e| TradeStoreError::Io { path: path.to_path_buf(), source: e })?;

    if data.len() < HEADER_SIZE {
        return Err(TradeStoreError::Truncated { expected: 0 });
    }
    if data[0] != MAGIC {
        return Err(TradeStoreError::BadMagic(data[0]));
    }
    if data[1] != VERSION {
        return Err(TradeStoreError::UnsupportedVersion(data[1]));
    }

    let trade_count = u32::from_le_bytes(data[2..6].try_into().unwrap()) as usize;
    let stored_crc = u32::from_le_bytes(data[6..10].try_into().unwrap());
    let payload = &data[HEADER_SIZE..];
    let expected_len = trade_count * TRADE_SIZE;

    if payload.len() < expected_len {
        return Err(TradeStoreError::Truncated { expected: trade_count });
    }

    let computed_crc = crc32fast::hash(&payload[..expected_len]);
    if computed_crc != stored_crc {
        return Err(TradeStoreError::CrcMismatch { stored: stored_crc, computed: computed_crc });
    }

    // Deserialize field-by-field — avoids any alignment assumptions on the raw bytes
    let mut trades = Vec::with_capacity(trade_count);
    for i in 0..trade_count {
        let offset = i * TRADE_SIZE;
        let chunk: &[u8; TRADE_SIZE] = payload[offset..offset + TRADE_SIZE].try_into().unwrap();
        trades.push(trade_from_bytes(chunk));
    }

    Ok(trades)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Trade;

    fn sample_trades(n: usize) -> Vec<Trade> {
        (0..n)
            .map(|i| Trade {
                timestamp_ms: 1_700_000_000_000 + (i as i64 * 100),
                price: 30_000.0 + i as f64 * 0.1,
                quantity: 0.01 * (i + 1) as f64,
                trade_id: 1_000_000 + i as u64,
                is_buyer_maker: (i % 2) as u8,
                _pad: [0u8; 7],
            })
            .collect()
    }

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join("trade_store_test_roundtrip");
        let path = dir.join("test.bin");
        let trades = sample_trades(100);
        write_trades(&path, &trades).unwrap();
        let loaded = read_trades(&path).unwrap();
        assert_eq!(trades.len(), loaded.len());
        for (a, b) in trades.iter().zip(loaded.iter()) {
            assert_eq!(a.timestamp_ms, b.timestamp_ms);
            assert_eq!(a.price, b.price);
            assert_eq!(a.quantity, b.quantity);
            assert_eq!(a.trade_id, b.trade_id);
            assert_eq!(a.is_buyer_maker, b.is_buyer_maker);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_trades() {
        let dir = std::env::temp_dir().join("trade_store_test_empty");
        let path = dir.join("empty.bin");
        write_trades(&path, &[]).unwrap();
        let loaded = read_trades(&path).unwrap();
        assert!(loaded.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_magic() {
        let dir = std::env::temp_dir().join("trade_store_test_magic");
        let path = dir.join("bad.bin");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, &[0x00u8, 0x01, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap();
        assert!(matches!(read_trades(&path), Err(TradeStoreError::BadMagic(0x00))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn crc_corruption() {
        let dir = std::env::temp_dir().join("trade_store_test_crc");
        let path = dir.join("corrupt.bin");
        let trades = sample_trades(10);
        write_trades(&path, &trades).unwrap();
        // Flip a byte in the payload
        let mut data = std::fs::read(&path).unwrap();
        data[HEADER_SIZE] ^= 0xFF;
        std::fs::write(&path, &data).unwrap();
        assert!(matches!(read_trades(&path), Err(TradeStoreError::CrcMismatch { .. })));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn truncated() {
        let dir = std::env::temp_dir().join("trade_store_test_trunc");
        let path = dir.join("trunc.bin");
        std::fs::create_dir_all(&dir).unwrap();
        // Valid header claiming 100 trades, but empty payload
        let mut buf = vec![MAGIC, VERSION];
        buf.extend_from_slice(&100u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes()); // CRC doesn't matter, truncated check comes first
        std::fs::write(&path, &buf).unwrap();
        assert!(matches!(read_trades(&path), Err(TradeStoreError::Truncated { expected: 100 })));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn trade_size_assertion() {
        assert_eq!(std::mem::size_of::<Trade>(), TRADE_SIZE);
    }
}
