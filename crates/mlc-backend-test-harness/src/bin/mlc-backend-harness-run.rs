//! Run every scenario in `mlc_backend_test_harness::scenarios` and print a
//! pass/fail report. Exit code 0 = all green, 1 = at least one failure.
//!
//! IMPORTANT: this runner uses a **synchronous** `main` because
//! `DataBridge::new()` itself spins a private tokio runtime via `block_on`,
//! and nesting that inside `#[tokio::main]` panics ("Cannot start a runtime
//! from within a runtime"). The async scenarios drive their own short-lived
//! runtime, separate from the bridge's.

use mlc_backend_test_harness::scenarios;

fn main() {
    let mut passes = 0usize;
    let mut fails: Vec<(String, String)> = Vec::new();

    println!("== mlc-backend-test-harness ==");
    println!();
    println!("-- sync scenarios --");
    for (name, f) in scenarios::sync_scenarios() {
        match f() {
            Ok(()) => {
                println!("PASS  {name}");
                passes += 1;
            }
            Err(e) => {
                println!("FAIL  {name}  >>  {}", e.message);
                fails.push((name.to_string(), e.message.clone()));
            }
        }
    }

    println!();
    println!("-- async scenarios --");
    // CRITICAL: BackendHarness::fresh() internally constructs DataBridge,
    // which itself calls `Runtime::new()` via block_on. That panics if it
    // runs inside another tokio runtime. So: build harnesses HERE (sync
    // context), then hand them off to a dedicated runtime that only does
    // HTTP work.
    let harness_a = mlc_backend_test_harness::BackendHarness::fresh();

    let http_rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            println!("FAIL  http runtime build: {e}");
            std::process::exit(2);
        }
    };

    // agent_state_apps_with_and_without_core consumes a BackendHarness so
    // the bridge runtime is already alive on its own thread.
    match http_rt.block_on(scenarios::agent_state_apps_with_and_without_core(harness_a)) {
        Ok(()) => {
            println!("PASS  agent_state_apps_with_and_without_core");
            passes += 1;
        }
        Err(e) => {
            println!("FAIL  agent_state_apps_with_and_without_core  >>  {}", e.message);
            fails.push(("agent_state_apps_with_and_without_core".to_string(), e.message.clone()));
        }
    }

    // daemon_e2e spawns an external process, so it doesn't need a
    // pre-built BackendHarness.
    match http_rt.block_on(scenarios::daemon_e2e()) {
        Ok(()) => {
            println!("PASS  daemon_e2e");
            passes += 1;
        }
        Err(e) => {
            println!("FAIL  daemon_e2e  >>  {}", e.message);
            fails.push(("daemon_e2e".to_string(), e.message.clone()));
        }
    }

    // Extended daemon endpoints (bubble CRUD, presets, templates, etc).
    match http_rt.block_on(scenarios::daemon_extended_e2e()) {
        Ok(()) => {
            println!("PASS  daemon_extended_e2e");
            passes += 1;
        }
        Err(e) => {
            println!("FAIL  daemon_extended_e2e  >>  {}", e.message);
            fails.push(("daemon_extended_e2e".to_string(), e.message.clone()));
        }
    }

    // mlc CLI binary smoke (spawns daemon, runs subcommands).
    match http_rt.block_on(scenarios::cli_binary_smoke()) {
        Ok(()) => {
            println!("PASS  cli_binary_smoke");
            passes += 1;
        }
        Err(e) => {
            println!("FAIL  cli_binary_smoke  >>  {}", e.message);
            fails.push(("cli_binary_smoke".to_string(), e.message.clone()));
        }
    }

    println!();
    println!("== summary: {passes} pass, {} fail ==", fails.len());
    if !fails.is_empty() {
        println!();
        println!("Failures:");
        for (n, m) in &fails {
            println!("  - {n}: {m}");
        }
        std::process::exit(1);
    }
}
