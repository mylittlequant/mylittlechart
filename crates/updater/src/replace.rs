//! Binary self-replacement and process restart.
//!
//! Ported from atol-ecommerce/crates/kkt-agent/src/ws_client.rs (handle_update_binary)
//! and lib.rs (wait_for_parent_exit).

/// Write new binary data to disk and replace the current executable.
///
/// Sequence:
/// 1. Write data to `{current_exe}.new`
/// 2. Remove stale `{current_exe}.old` if present
/// 3. Rename `{current_exe}` → `{current_exe}.old`
/// 4. Rename `{current_exe}.new` → `{current_exe}`
///
/// On failure at step 3-4, attempts rollback from `.old`.
pub fn self_replace(binary_data: &[u8]) -> Result<(), String> {
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("Cannot determine current exe: {}", e))?;

    let new_path = current_exe.with_extension("exe.new");
    let old_path = current_exe.with_extension("exe.old");

    // 1. Write new binary
    std::fs::write(&new_path, binary_data)
        .map_err(|e| format!("Failed to write new binary: {}", e))?;

    // 2. Remove stale .old
    let _ = std::fs::remove_file(&old_path);

    // 3. Rename current → .old
    if let Err(e) = std::fs::rename(&current_exe, &old_path) {
        // Cleanup: remove the .new file
        let _ = std::fs::remove_file(&new_path);
        return Err(format!("Failed to rename current exe to .old: {}", e));
    }

    // 4. Rename .new → current
    if let Err(e) = std::fs::rename(&new_path, &current_exe) {
        // Rollback: restore .old → current
        let _ = std::fs::rename(&old_path, &current_exe);
        return Err(format!("Failed to rename .new to current: {}", e));
    }

    log::info!("Binary replaced successfully: {}", current_exe.display());
    Ok(())
}

/// Spawn the new binary with `--wait-pid <our_pid>` (and optionally
/// `--wait-port <port>`) then exit this process.
///
/// `server_port` should be the port that the current process is listening on
/// (e.g. 17420).  The new process will probe that port until it is free before
/// proceeding, avoiding the Windows TCP TIME_WAIT / IOCP hold issue.
pub fn spawn_and_exit(server_port: Option<u16>) -> Result<(), String> {
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("Cannot determine current exe: {}", e))?;

    let my_pid = std::process::id();

    // Collect original args (skip argv[0]), filter out any existing --wait-pid
    // and --wait-port flags so we don't accumulate duplicates across restarts.
    let mut skip_next = false;
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| {
            if skip_next {
                skip_next = false;
                return false;
            }
            if a == "--wait-pid" || a == "--wait-port" {
                skip_next = true;
                return false;
            }
            // Also filter combined forms like "--wait-pid=123"
            if a.starts_with("--wait-pid=") || a.starts_with("--wait-port=") {
                return false;
            }
            true
        })
        .collect();

    log::info!(
        "Spawning new process: {} --wait-pid {} {:?}",
        current_exe.display(),
        my_pid,
        server_port.map(|p| format!("--wait-port {}", p)).unwrap_or_default()
    );

    let mut cmd = std::process::Command::new(&current_exe);
    cmd.args(&args);
    cmd.arg("--wait-pid").arg(my_pid.to_string());

    if let Some(port) = server_port {
        cmd.arg("--wait-port").arg(port.to_string());
    }

    cmd.spawn()
        .map_err(|e| format!("Failed to spawn new process: {}", e))?;

    std::process::exit(0);
}

/// Called at the very top of main(). If `--wait-pid <PID>` is present in args,
/// wait for that process to exit before proceeding. This allows the old binary
/// to release file handles and ports.
///
/// If `--wait-port <PORT>` is also present, after the PID disappears this
/// function probes that TCP port until it can be bound (i.e. the old process
/// has released its listener).  This fixes the Windows IOCP / TIME_WAIT issue
/// where the TCP socket lingers for 100+ seconds after `process::exit(0)`.
pub fn wait_for_parent_exit() {
    let args: Vec<String> = std::env::args().collect();

    // Parse --wait-pid
    let pid_str = args.iter()
        .position(|a| a == "--wait-pid")
        .and_then(|i| args.get(i + 1))
        .cloned();

    // Parse --wait-port
    let port: Option<u16> = args.iter()
        .position(|a| a == "--wait-port")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u16>().ok());

    let pid_str = match pid_str {
        Some(p) => p,
        None => return, // Not a restart — proceed normally
    };

    log::info!("Waiting for parent process {} to exit...", pid_str);

    #[cfg(target_os = "windows")]
    {
        // Phase 1: wait for the PID to disappear from tasklist.
        let mut pid_gone = false;
        for _ in 0..60 {
            let output = std::process::Command::new("tasklist")
                .args(&["/FI", &format!("PID eq {}", pid_str), "/NH"])
                .output();

            match output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    if !stdout.contains(&pid_str) {
                        log::info!("Parent process {} has exited", pid_str);
                        pid_gone = true;
                        break;
                    }
                }
                Err(_) => {
                    // tasklist unavailable — proceed anyway
                    log::warn!("tasklist unavailable, proceeding without wait");
                    return;
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(500));
        }

        if !pid_gone {
            log::warn!("Timed out waiting for parent process {} to exit", pid_str);
        }

        // Phase 2: if a port was given, probe until it's free (or timeout).
        // Windows IOCP may still hold TCP sockets for a significant time after
        // the process PID disappears.
        if let Some(p) = port {
            log::info!("Probing port {} until free (max 60s)...", p);
            let addr = std::net::SocketAddr::from(([127, 0, 0, 1], p));
            let mut port_free = false;
            for attempt in 0..120 {
                match std::net::TcpListener::bind(addr) {
                    Ok(_listener) => {
                        // Listener is dropped immediately here — we just needed
                        // to confirm the port is bindable.
                        log::info!("Port {} is free after {} probe(s)", p, attempt + 1);
                        port_free = true;
                        break;
                    }
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                }
            }
            if !port_free {
                log::warn!("Port {} still busy after 60s — proceeding anyway", p);
            }
        } else {
            // No port given — fall back to the old fixed 3s delay.
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Phase 1: wait for the PID to disappear via /proc.
        use std::path::Path;
        let mut pid_gone = false;
        for _ in 0..60 {
            let proc_path = format!("/proc/{}", pid_str);
            if !Path::new(&proc_path).exists() {
                log::info!("Parent process {} has exited", pid_str);
                pid_gone = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }

        if !pid_gone {
            log::warn!("Timed out waiting for parent process {} to exit", pid_str);
        }

        // Phase 2: probe the port until free (or timeout).
        if let Some(p) = port {
            log::info!("Probing port {} until free (max 60s)...", p);
            let addr = std::net::SocketAddr::from(([127, 0, 0, 1], p));
            let mut port_free = false;
            for attempt in 0..120 {
                match std::net::TcpListener::bind(addr) {
                    Ok(_listener) => {
                        log::info!("Port {} is free after {} probe(s)", p, attempt + 1);
                        port_free = true;
                        break;
                    }
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                }
            }
            if !port_free {
                log::warn!("Port {} still busy after 60s — proceeding anyway", p);
            }
        }
    }
}
