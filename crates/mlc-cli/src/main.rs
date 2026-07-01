//! `mlc` — CLI for the mlc backend.
//!
//! Default mode talks HTTP to a running `mlc-backend-daemon` on
//! `127.0.0.1:19470` (override with `--remote URL` or `MLC_REMOTE` env).
//! Every Mode-2 control-plane endpoint has a flat sub-command. Output is
//! human-formatted unless `--json` is passed.

use std::process::ExitCode;
use std::time::Duration;

use anyhow::{anyhow, Context};
use clap::{Args, Parser, Subcommand};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const DEFAULT_REMOTE: &str = "http://127.0.0.1:19470";

#[derive(Parser)]
#[command(name = "mlc", version, about = "mylittlechart CLI", long_about = None)]
struct Cli {
    /// Daemon URL. Default $MLC_REMOTE then http://127.0.0.1:19470.
    #[arg(long, global = true)]
    remote: Option<String>,

    /// Emit raw JSON instead of human-formatted output.
    #[arg(long, global = true)]
    json: bool,

    /// HTTP request timeout in seconds (default 10).
    #[arg(long, global = true, default_value_t = 10)]
    timeout: u64,

    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// GET /health
    Health,

    /// Bubble lifecycle and bars.
    Bubble {
        #[command(subcommand)]
        action: BubbleCmd,
    },
    /// Preset store.
    Preset {
        #[command(subcommand)]
        action: PresetCmd,
    },
    /// Watchlist read.
    Watchlist {
        #[command(subcommand)]
        action: WatchlistCmd,
    },
    /// Template summary.
    Template {
        #[command(subcommand)]
        action: TemplateCmd,
    },
    /// Indicator read.
    Indicator {
        #[command(subcommand)]
        action: IndicatorCmd,
    },
    /// Drawing (per-bubble DrawingManager) read.
    Drawing {
        #[command(subcommand)]
        action: DrawingCmd,
    },
    /// AppSession lifecycle.
    Session {
        #[command(subcommand)]
        action: SessionCmd,
    },
    /// Mini-ticker cache.
    Ticker {
        #[command(subcommand)]
        action: TickerCmd,
    },
    /// Exchange catalogue.
    Exchange {
        #[command(subcommand)]
        action: ExchangeCmd,
    },
    /// DataBridge requests.
    Bridge {
        #[command(subcommand)]
        action: BridgeCmd,
    },
    /// Trading-manager read.
    Trading {
        #[command(subcommand)]
        action: TradingCmd,
    },
    /// Stream BubbleEvent SSE to stdout.
    Events {
        /// Stop after N events (0 = no cap).
        #[arg(long, default_value_t = 0)]
        limit: usize,
        /// Filter by event variant name (case-insensitive substring).
        #[arg(long)]
        filter: Option<String>,
    },
    /// Spawn the daemon binary in the background (best-effort).
    DaemonSpawn(DaemonSpawnArgs),
}

#[derive(Subcommand)]
enum BubbleCmd {
    Ls,
    Create(BubbleCreate),
    Rm { bid: u64 },
    Info { bid: u64 },
    Bars {
        bid: u64,
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
}

#[derive(Args)]
struct BubbleCreate {
    symbol: String,
    #[arg(long, default_value = "binance")]
    exchange: String,
    #[arg(long, default_value = "S")]
    account: String,
    #[arg(long, default_value = "1m")]
    tf: String,
    /// Color as RGBA floats "r,g,b,a" (each 0-1).
    #[arg(long)]
    color: Option<String>,
}

#[derive(Subcommand)]
enum PresetCmd {
    Ls,
    Show { id: String },
    Rm { id: String },
}

#[derive(Subcommand)]
enum WatchlistCmd {
    Ls,
}

#[derive(Subcommand)]
enum TemplateCmd {
    Summary,
}

#[derive(Subcommand)]
enum IndicatorCmd {
    Ls,
}

#[derive(Subcommand)]
enum DrawingCmd {
    Ls,
}

#[derive(Subcommand)]
enum SessionCmd {
    Ls,
    Register {
        #[arg(long)]
        label: Option<String>,
    },
    Rm { app_id: String },
    Heartbeat { app_id: String },
    Reap {
        #[arg(long, default_value_t = 60)]
        idle: u64,
    },
}

#[derive(Subcommand)]
enum TickerCmd {
    Ls,
    Get { key: String },
}

#[derive(Subcommand)]
enum ExchangeCmd {
    Ls,
}

#[derive(Subcommand)]
enum BridgeCmd {
    /// POST /v1/bridge/request-bars
    RequestBars {
        symbol: String,
        #[arg(long, default_value = "binance")]
        exchange: String,
        #[arg(long, default_value = "S")]
        account: String,
        #[arg(long, default_value = "1m")]
        tf: String,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        total: Option<u32>,
        #[arg(long, default_value_t = false)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum TradingCmd {
    Connectors,
}

#[derive(Args)]
struct DaemonSpawnArgs {
    #[arg(long, default_value_t = 19470)]
    port: u16,
    /// Path to the daemon binary. Defaults to ./target/debug/mlc-backend-daemon[.exe].
    #[arg(long)]
    binary: Option<String>,
    /// Wait this many seconds for /health to answer.
    #[arg(long, default_value_t = 5)]
    wait: u64,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("mlc: tokio runtime: {e}");
            return ExitCode::from(2);
        }
    };
    match rt.block_on(run(cli)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mlc: error: {e:#}");
            ExitCode::from(1)
        }
    }
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    let remote = cli
        .remote
        .or_else(|| std::env::var("MLC_REMOTE").ok())
        .unwrap_or_else(|| DEFAULT_REMOTE.to_string());
    let remote = remote.trim_end_matches('/').to_string();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(cli.timeout))
        .build()
        .context("build http client")?;

    let ctx = Ctx {
        client,
        remote,
        json: cli.json,
    };

    match cli.cmd {
        Command::Health => cmd_health(&ctx).await,
        Command::Bubble { action } => cmd_bubble(&ctx, action).await,
        Command::Preset { action } => cmd_preset(&ctx, action).await,
        Command::Watchlist { action } => cmd_watchlist(&ctx, action).await,
        Command::Template { action } => cmd_template(&ctx, action).await,
        Command::Indicator { action } => cmd_indicator(&ctx, action).await,
        Command::Drawing { action } => cmd_drawing(&ctx, action).await,
        Command::Session { action } => cmd_session(&ctx, action).await,
        Command::Ticker { action } => cmd_ticker(&ctx, action).await,
        Command::Exchange { action } => cmd_exchange(&ctx, action).await,
        Command::Bridge { action } => cmd_bridge(&ctx, action).await,
        Command::Trading { action } => cmd_trading(&ctx, action).await,
        Command::Events { limit, filter } => cmd_events(&ctx, limit, filter).await,
        Command::DaemonSpawn(args) => cmd_daemon_spawn(&ctx, args).await,
    }
}

struct Ctx {
    client: reqwest::Client,
    remote: String,
    json: bool,
}

impl Ctx {
    fn url(&self, path: &str) -> String {
        format!("{}{}", self.remote, path)
    }

    async fn get_value(&self, path: &str) -> anyhow::Result<Value> {
        let r = self
            .client
            .get(self.url(path))
            .send()
            .await
            .with_context(|| format!("GET {path}"))?;
        let st = r.status();
        let body = r.text().await.unwrap_or_default();
        if !st.is_success() {
            return Err(anyhow!("GET {path}: HTTP {st}\n{body}"));
        }
        if body.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&body).with_context(|| format!("decode {path}: body was: {body}"))
    }

    async fn post_value(&self, path: &str, body: Value) -> anyhow::Result<Value> {
        let r = self
            .client
            .post(self.url(path))
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {path}"))?;
        let st = r.status();
        let txt = r.text().await.unwrap_or_default();
        if !st.is_success() {
            return Err(anyhow!("POST {path}: HTTP {st}\n{txt}"));
        }
        if txt.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&txt).with_context(|| format!("decode {path}: body was: {txt}"))
    }

    async fn delete(&self, path: &str) -> anyhow::Result<()> {
        let r = self
            .client
            .delete(self.url(path))
            .send()
            .await
            .with_context(|| format!("DELETE {path}"))?;
        let st = r.status();
        if !st.is_success() && st.as_u16() != 204 {
            let txt = r.text().await.unwrap_or_default();
            return Err(anyhow!("DELETE {path}: HTTP {st}\n{txt}"));
        }
        Ok(())
    }

    fn emit(&self, v: &Value) -> anyhow::Result<()> {
        if self.json {
            println!("{}", serde_json::to_string_pretty(v)?);
        } else {
            print_human(v);
        }
        Ok(())
    }
}

fn print_human(v: &Value) {
    match v {
        Value::Object(map) => {
            for (k, vv) in map {
                match vv {
                    Value::String(s) => println!("{k}: {s}"),
                    Value::Number(n) => println!("{k}: {n}"),
                    Value::Bool(b) => println!("{k}: {b}"),
                    Value::Null => println!("{k}: -"),
                    Value::Array(arr) => println!("{k}: [{} items]", arr.len()),
                    Value::Object(o) => println!("{k}: {{{} fields}}", o.len()),
                }
            }
        }
        Value::Array(arr) => {
            for (i, item) in arr.iter().enumerate() {
                match item {
                    Value::Object(_) => {
                        println!("[{i}]");
                        print_human(item);
                    }
                    other => println!("- {other}"),
                }
            }
        }
        other => println!("{other}"),
    }
}

// =============================================================================
// Command handlers
// =============================================================================

async fn cmd_health(ctx: &Ctx) -> anyhow::Result<()> {
    let v = ctx.get_value("/health").await?;
    ctx.emit(&v)
}

async fn cmd_bubble(ctx: &Ctx, action: BubbleCmd) -> anyhow::Result<()> {
    match action {
        BubbleCmd::Ls => {
            let v = ctx.get_value("/v1/bubbles").await?;
            ctx.emit(&v)
        }
        BubbleCmd::Create(c) => {
            let mut body = serde_json::json!({
                "symbol": c.symbol,
                "exchange": c.exchange,
                "account_type": c.account,
                "timeframe": c.tf,
            });
            if let Some(col) = c.color {
                let parts: Vec<f64> = col
                    .split(',')
                    .map(|s| s.trim().parse().unwrap_or(1.0))
                    .collect();
                if parts.len() == 4 {
                    body["color"] = serde_json::json!(parts);
                }
            }
            let v = ctx.post_value("/v1/bubbles", body).await?;
            ctx.emit(&v)
        }
        BubbleCmd::Rm { bid } => {
            ctx.delete(&format!("/v1/bubbles/{bid}")).await?;
            if !ctx.json {
                println!("bubble {bid} removed");
            }
            Ok(())
        }
        BubbleCmd::Info { bid } => {
            let v = ctx.get_value(&format!("/v1/bubbles/{bid}")).await?;
            ctx.emit(&v)
        }
        BubbleCmd::Bars { bid, limit } => {
            let v = ctx
                .get_value(&format!("/v1/bubbles/{bid}/bars?limit={limit}"))
                .await?;
            ctx.emit(&v)
        }
    }
}

async fn cmd_preset(ctx: &Ctx, action: PresetCmd) -> anyhow::Result<()> {
    match action {
        PresetCmd::Ls => ctx.emit(&ctx.get_value("/v1/presets").await?),
        PresetCmd::Show { id } => {
            ctx.emit(&ctx.get_value(&format!("/v1/presets/{id}")).await?)
        }
        PresetCmd::Rm { id } => {
            ctx.delete(&format!("/v1/presets/{id}")).await?;
            if !ctx.json {
                println!("preset {id} removed");
            }
            Ok(())
        }
    }
}

async fn cmd_watchlist(ctx: &Ctx, action: WatchlistCmd) -> anyhow::Result<()> {
    match action {
        WatchlistCmd::Ls => ctx.emit(&ctx.get_value("/v1/watchlists").await?),
    }
}

async fn cmd_template(ctx: &Ctx, action: TemplateCmd) -> anyhow::Result<()> {
    match action {
        TemplateCmd::Summary => ctx.emit(&ctx.get_value("/v1/templates/summary").await?),
    }
}

async fn cmd_indicator(ctx: &Ctx, action: IndicatorCmd) -> anyhow::Result<()> {
    match action {
        IndicatorCmd::Ls => ctx.emit(&ctx.get_value("/v1/indicators").await?),
    }
}

async fn cmd_drawing(ctx: &Ctx, action: DrawingCmd) -> anyhow::Result<()> {
    match action {
        DrawingCmd::Ls => ctx.emit(&ctx.get_value("/v1/drawings").await?),
    }
}

async fn cmd_session(ctx: &Ctx, action: SessionCmd) -> anyhow::Result<()> {
    match action {
        SessionCmd::Ls => ctx.emit(&ctx.get_value("/v1/apps").await?),
        SessionCmd::Register { label } => {
            let body = serde_json::json!({ "label": label });
            ctx.emit(&ctx.post_value("/v1/apps", body).await?)
        }
        SessionCmd::Rm { app_id } => {
            ctx.delete(&format!("/v1/apps/{app_id}")).await?;
            if !ctx.json {
                println!("session {app_id} removed");
            }
            Ok(())
        }
        SessionCmd::Heartbeat { app_id } => {
            ctx.post_value(&format!("/v1/apps/{app_id}/heartbeat"), Value::Null)
                .await?;
            if !ctx.json {
                println!("session {app_id} heartbeat ok");
            }
            Ok(())
        }
        SessionCmd::Reap { idle } => {
            let body = serde_json::json!({ "idle_secs": idle });
            ctx.emit(&ctx.post_value("/v1/apps/_reap", body).await?)
        }
    }
}

async fn cmd_ticker(ctx: &Ctx, action: TickerCmd) -> anyhow::Result<()> {
    match action {
        TickerCmd::Ls => ctx.emit(&ctx.get_value("/v1/mini-ticker").await?),
        TickerCmd::Get { key } => {
            ctx.emit(&ctx.get_value(&format!("/v1/mini-ticker/{key}")).await?)
        }
    }
}

async fn cmd_exchange(ctx: &Ctx, action: ExchangeCmd) -> anyhow::Result<()> {
    match action {
        ExchangeCmd::Ls => ctx.emit(&ctx.get_value("/v1/exchanges").await?),
    }
}

async fn cmd_bridge(ctx: &Ctx, action: BridgeCmd) -> anyhow::Result<()> {
    match action {
        BridgeCmd::RequestBars {
            symbol,
            exchange,
            account,
            tf,
            limit,
            total,
            force,
        } => {
            let body = serde_json::json!({
                "symbol": symbol,
                "exchange": exchange,
                "account_type": account,
                "timeframe": tf,
                "limit": limit,
                "total_bars": total,
                "force": force,
            });
            ctx.emit(&ctx.post_value("/v1/bridge/request-bars", body).await?)
        }
    }
}

async fn cmd_trading(ctx: &Ctx, action: TradingCmd) -> anyhow::Result<()> {
    match action {
        TradingCmd::Connectors => {
            ctx.emit(&ctx.get_value("/v1/trading/connectors").await?)
        }
    }
}

async fn cmd_events(
    ctx: &Ctx,
    limit: usize,
    filter: Option<String>,
) -> anyhow::Result<()> {
    use eventsource_stream::Eventsource;

    let resp = ctx
        .client
        .get(ctx.url("/v1/events"))
        .send()
        .await
        .context("connect /v1/events")?;
    if !resp.status().is_success() {
        return Err(anyhow!("/v1/events: HTTP {}", resp.status()));
    }
    let mut stream = resp.bytes_stream().eventsource();

    let mut seen = 0usize;
    let filter_lc = filter.map(|s| s.to_lowercase());
    while let Some(event) = stream.next().await {
        match event {
            Ok(ev) => {
                let name = ev.event.clone();
                if let Some(f) = &filter_lc {
                    if !name.to_lowercase().contains(f) {
                        continue;
                    }
                }
                if ctx.json {
                    let merged = serde_json::json!({
                        "event": name,
                        "data": serde_json::from_str::<Value>(&ev.data).unwrap_or(Value::String(ev.data.clone())),
                    });
                    println!("{}", serde_json::to_string(&merged)?);
                } else {
                    println!("[{name}] {}", ev.data);
                }
                seen += 1;
                if limit > 0 && seen >= limit {
                    break;
                }
            }
            Err(e) => {
                eprintln!("events stream error: {e}");
                break;
            }
        }
    }
    Ok(())
}

async fn cmd_daemon_spawn(_ctx: &Ctx, args: DaemonSpawnArgs) -> anyhow::Result<()> {
    let exe_name = if cfg!(windows) {
        "mlc-backend-daemon.exe"
    } else {
        "mlc-backend-daemon"
    };
    let binary = match args.binary {
        Some(b) => std::path::PathBuf::from(b),
        None => {
            let cwd = std::env::current_dir()?;
            let debug = cwd.join("target/debug").join(exe_name);
            let release = cwd.join("target/release").join(exe_name);
            if debug.exists() {
                debug
            } else if release.exists() {
                release
            } else {
                return Err(anyhow!(
                    "daemon binary not found at {debug:?} or {release:?} — pass --binary"
                ));
            }
        }
    };

    let mut cmd = std::process::Command::new(&binary);
    cmd.env("MLC_BACKEND_DAEMON_PORT", args.port.to_string());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS so it survives parent exit.
        cmd.creation_flags(0x0000_0008);
    }
    let child = cmd.spawn().with_context(|| format!("spawn {binary:?}"))?;
    let pid = child.id();

    // Wait for /health
    let base = format!("http://127.0.0.1:{}", args.port);
    let url = format!("{base}/health");
    let deadline = web_time::Instant::now() + Duration::from_secs(args.wait);
    let client = reqwest::Client::new();
    loop {
        if let Ok(r) = client.get(&url).send().await {
            if r.status().is_success() {
                let body = r.text().await.unwrap_or_default();
                if std::env::var("MLC_DAEMON_SPAWN_QUIET").is_err() {
                    println!("daemon up: pid={pid} url={base} health={body}");
                }
                return Ok(());
            }
        }
        if web_time::Instant::now() > deadline {
            return Err(anyhow!(
                "daemon spawned (pid={pid}) but /health did not respond within {}s",
                args.wait
            ));
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
}

// Silence unused warning in non-windows builds.
#[allow(dead_code)]
#[derive(Serialize, Deserialize)]
struct Probe;
