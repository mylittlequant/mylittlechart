use crate::{format, Bar, BarStoreError};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

enum BarWriteCmd {
    Write {
        path: PathBuf,
        bars: Arc<Vec<Bar>>,
    },
    Flush(tokio::sync::oneshot::Sender<()>),
}

/// Async bar cache writer. Send writes here; they execute on the tokio runtime.
#[derive(Clone)]
pub struct BarStoreHandle {
    tx: mpsc::UnboundedSender<BarWriteCmd>,
    pub bars_dir: PathBuf,
}

impl BarStoreHandle {
    /// Create a new `BarStoreHandle` and spawn the writer task on the given runtime.
    pub fn new(bars_dir: PathBuf, runtime: &tokio::runtime::Runtime) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<BarWriteCmd>();

        runtime.spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    BarWriteCmd::Write { path, bars } => {
                        let _ = tokio::task::spawn_blocking(move || {
                            if let Err(e) = format::write_bars(&path, &bars) {
                                eprintln!("[BarStore] write error {:?}: {}", path, e);
                            }
                        })
                        .await;
                    }
                    BarWriteCmd::Flush(reply) => {
                        let _ = reply.send(());
                    }
                }
            }
        });

        Self { tx, bars_dir }
    }

    /// Build the file path for a given exchange/symbol/timeframe.
    pub fn file_path(&self, exchange: &str, symbol: &str, timeframe: &str) -> PathBuf {
        let safe_symbol = symbol.replace('/', "-");
        self.bars_dir
            .join(exchange.to_lowercase())
            .join(format!("{}_{}.bin", safe_symbol, timeframe))
    }

    /// Queue an async write. Never blocks the caller.
    pub fn write_async(
        &self,
        exchange: &str,
        symbol: &str,
        timeframe: &str,
        bars: Arc<Vec<Bar>>,
    ) {
        let path = self.file_path(exchange, symbol, timeframe);
        let _ = self.tx.send(BarWriteCmd::Write { path, bars });
    }

    /// Load bars from disk for the given keys.
    ///
    /// Returns successfully loaded entries. Errors are logged and skipped.
    /// `NotFound` errors are silently ignored (expected on first run).
    pub fn load_many(
        &self,
        keys: &[(&str, &str, &str)],
    ) -> Vec<(String, String, String, Vec<Bar>)> {
        let mut result = Vec::with_capacity(keys.len());
        for &(exchange, symbol, timeframe) in keys {
            let path = self.file_path(exchange, symbol, timeframe);
            match format::read_bars(&path) {
                Ok(bars) => {
                    eprintln!("[BarStore] loaded {} bars from {:?}", bars.len(), path);
                    result.push((
                        exchange.to_string(),
                        symbol.to_string(),
                        timeframe.to_string(),
                        bars,
                    ));
                }
                Err(e) => {
                    // NotFound is expected on first run — skip silently
                    if !matches!(&e, BarStoreError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound)
                    {
                        eprintln!("[BarStore] skip {:?}: {}", path, e);
                    }
                }
            }
        }
        result
    }

    /// Flush all queued writes. Blocks the calling thread until the queue drains.
    ///
    /// Only call from the main thread during shutdown (`save_all()`).
    pub fn flush_sync(&self) {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let _ = self.tx.send(BarWriteCmd::Flush(reply_tx));
        // blocking_recv is safe to call from a non-async context (shutdown path)
        let _ = reply_rx.blocking_recv();
    }
}
