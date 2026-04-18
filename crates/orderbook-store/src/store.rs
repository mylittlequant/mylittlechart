use crate::{format, TimedSnapshot, OrderbookStoreError};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

enum OrderbookWriteCmd {
    Write {
        path: PathBuf,
        snapshots: Arc<Vec<TimedSnapshot>>,
    },
    Flush(tokio::sync::oneshot::Sender<()>),
}

/// Async orderbook snapshot writer. Send writes here; they execute on the tokio runtime.
#[derive(Clone)]
pub struct OrderbookStoreHandle {
    tx: mpsc::UnboundedSender<OrderbookWriteCmd>,
    pub orderbook_dir: PathBuf,
}

impl OrderbookStoreHandle {
    /// Create a new `OrderbookStoreHandle` and spawn the writer task on the given runtime.
    pub fn new(orderbook_dir: PathBuf, runtime: &tokio::runtime::Runtime) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<OrderbookWriteCmd>();

        runtime.spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    OrderbookWriteCmd::Write { path, snapshots } => {
                        let _ = tokio::task::spawn_blocking(move || {
                            if let Err(e) = format::write_snapshots(&path, &snapshots) {
                                eprintln!("[OrderbookStore] write error {:?}: {}", path, e);
                            }
                        })
                        .await;
                    }
                    OrderbookWriteCmd::Flush(reply) => {
                        let _ = reply.send(());
                    }
                }
            }
        });

        Self { tx, orderbook_dir }
    }

    /// Build the file path for a given exchange/symbol/account_type.
    ///
    /// Format: `{orderbook_dir}/{exchange_lower}/{symbol}_{account_type}.bin`
    /// Example: `orderbook/binance/BTCUSDT_F.bin`
    pub fn file_path(&self, exchange: &str, symbol: &str, account_type: &str) -> PathBuf {
        let safe_symbol = symbol.replace('/', "-").replace(':', "_");
        let filename = if account_type.is_empty() || account_type == "S" {
            format!("{}.bin", safe_symbol)
        } else {
            format!("{}_{}.bin", safe_symbol, account_type)
        };
        self.orderbook_dir.join(exchange.to_lowercase()).join(filename)
    }

    /// Queue an async write. Never blocks the caller.
    pub fn write_async(
        &self,
        exchange: &str,
        symbol: &str,
        account_type: &str,
        snapshots: Arc<Vec<TimedSnapshot>>,
    ) {
        let path = self.file_path(exchange, symbol, account_type);
        let _ = self.tx.send(OrderbookWriteCmd::Write { path, snapshots });
    }

    /// Load snapshots from disk for the given key.
    ///
    /// Returns an empty `Vec` on `NotFound` (silent). Other errors are logged.
    pub fn load(
        &self,
        exchange: &str,
        symbol: &str,
        account_type: &str,
    ) -> Vec<TimedSnapshot> {
        let path = self.file_path(exchange, symbol, account_type);
        match format::read_snapshots(&path) {
            Ok(snaps) => {
                eprintln!("[OrderbookStore] loaded {} snapshots from {:?}", snaps.len(), path);
                snaps
            }
            Err(e) => {
                // NotFound is expected on first run — skip silently
                if !matches!(&e, OrderbookStoreError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound) {
                    eprintln!("[OrderbookStore] skip {:?}: {}", path, e);
                }
                vec![]
            }
        }
    }

    /// Delete the .bin cache file for a specific symbol/exchange/account_type.
    ///
    /// Returns `true` if a file was deleted, `false` if it did not exist.
    pub fn delete_file(&self, exchange: &str, symbol: &str, account_type: &str) -> bool {
        let path = self.file_path(exchange, symbol, account_type);
        match std::fs::remove_file(&path) {
            Ok(()) => {
                eprintln!("[OrderbookStore] Deleted: {}", path.display());
                true
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("[OrderbookStore] File not found (already clean): {}", path.display());
                false
            }
            Err(e) => {
                eprintln!("[OrderbookStore] Error deleting {}: {}", path.display(), e);
                false
            }
        }
    }

    /// Flush all queued writes. Blocks the calling thread until the queue drains.
    ///
    /// Only call from the main thread during shutdown.
    pub fn flush_sync(&self) {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let _ = self.tx.send(OrderbookWriteCmd::Flush(reply_tx));
        let _ = reply_rx.blocking_recv();
    }
}
