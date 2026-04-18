use crate::{format, Trade, TradeStoreError};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

enum TradeWriteCmd {
    Write {
        path: PathBuf,
        trades: Arc<Vec<Trade>>,
    },
    Flush(tokio::sync::oneshot::Sender<()>),
}

/// Async trade cache writer. Send writes here; they execute on the tokio runtime.
#[derive(Clone)]
pub struct TradeStoreHandle {
    tx: mpsc::UnboundedSender<TradeWriteCmd>,
    pub trades_dir: PathBuf,
}

impl TradeStoreHandle {
    /// Create a new `TradeStoreHandle` and spawn the writer task on the given runtime.
    pub fn new(trades_dir: PathBuf, runtime: &tokio::runtime::Runtime) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<TradeWriteCmd>();

        runtime.spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    TradeWriteCmd::Write { path, trades } => {
                        let _ = tokio::task::spawn_blocking(move || {
                            if let Err(e) = format::write_trades(&path, &trades) {
                                eprintln!("[TradeStore] write error {:?}: {}", path, e);
                            }
                        })
                        .await;
                    }
                    TradeWriteCmd::Flush(reply) => {
                        let _ = reply.send(());
                    }
                }
            }
        });

        Self { tx, trades_dir }
    }

    /// Build the file path for a given exchange/symbol/account_type.
    ///
    /// Format: `{trades_dir}/{exchange_lower}/{symbol}_{account_type}.bin`
    /// Example: `trades/binance/BTCUSDT_S.bin`
    pub fn file_path(&self, exchange: &str, symbol: &str, account_type: &str) -> PathBuf {
        let safe_symbol = symbol.replace('/', "-").replace(':', "_");
        let filename = if account_type.is_empty() || account_type == "S" {
            format!("{}.bin", safe_symbol)
        } else {
            format!("{}_{}.bin", safe_symbol, account_type)
        };
        self.trades_dir.join(exchange.to_lowercase()).join(filename)
    }

    /// Queue an async write. Never blocks the caller.
    pub fn write_async(
        &self,
        exchange: &str,
        symbol: &str,
        account_type: &str,
        trades: Arc<Vec<Trade>>,
    ) {
        let path = self.file_path(exchange, symbol, account_type);
        let _ = self.tx.send(TradeWriteCmd::Write { path, trades });
    }

    /// Load trades from disk for the given key.
    ///
    /// Returns `Ok(vec)` on success. `NotFound` is silently ignored (returns empty vec).
    /// Other errors are logged and return empty vec.
    pub fn load(
        &self,
        exchange: &str,
        symbol: &str,
        account_type: &str,
    ) -> Vec<Trade> {
        let path = self.file_path(exchange, symbol, account_type);
        match format::read_trades(&path) {
            Ok(trades) => {
                eprintln!("[TradeStore] loaded {} trades from {:?}", trades.len(), path);
                trades
            }
            Err(e) => {
                // NotFound is expected on first run — skip silently
                if !matches!(&e, TradeStoreError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound) {
                    eprintln!("[TradeStore] skip {:?}: {}", path, e);
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
                eprintln!("[TradeStore] Deleted: {}", path.display());
                true
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("[TradeStore] File not found (already clean): {}", path.display());
                false
            }
            Err(e) => {
                eprintln!("[TradeStore] Error deleting {}: {}", path.display(), e);
                false
            }
        }
    }

    /// Flush all queued writes. Blocks the calling thread until the queue drains.
    ///
    /// Only call from the main thread during shutdown.
    pub fn flush_sync(&self) {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let _ = self.tx.send(TradeWriteCmd::Flush(reply_tx));
        // blocking_recv is safe to call from a non-async context (shutdown path)
        let _ = reply_rx.blocking_recv();
    }
}
