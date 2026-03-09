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

/// Spawn the new binary with `--wait-pid <our_pid>` and exit this process.
pub fn spawn_and_exit() -> Result<(), String> {
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("Cannot determine current exe: {}", e))?;

    let my_pid = std::process::id();

    // Collect original args (skip argv[0]), filter out any existing --wait-pid
    let args: Vec<String> = std::env::args()
        .skip(1)
        .collect::<Vec<_>>()
        .into_iter()
        .filter(|a| !a.starts_with("--wait-pid"))
        .collect();

    log::info!("Spawning new process: {} --wait-pid {}", current_exe.display(), my_pid);

    std::process::Command::new(&current_exe)
        .args(&args)
        .arg("--wait-pid")
        .arg(my_pid.to_string())
        .spawn()
        .map_err(|e| format!("Failed to spawn new process: {}", e))?;

    std::process::exit(0);
}

/// Called at the very top of main(). If `--wait-pid <PID>` is present in args,
/// wait for that process to exit before proceeding. This allows the old binary
/// to release file handles and ports.
pub fn wait_for_parent_exit() {
    let args: Vec<String> = std::env::args().collect();
    let pid_str = args.iter()
        .position(|a| a == "--wait-pid")
        .and_then(|i| args.get(i + 1))
        .cloned();

    let pid_str = match pid_str {
        Some(p) => p,
        None => return, // Not a restart — proceed normally
    };

    log::info!("Waiting for parent process {} to exit...", pid_str);

    #[cfg(target_os = "windows")]
    {
        // Poll via tasklist
        for _ in 0..30 {
            let output = std::process::Command::new("tasklist")
                .args(&["/FI", &format!("PID eq {}", pid_str), "/NH"])
                .output();

            match output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    if !stdout.contains(&pid_str) {
                        log::info!("Parent process {} has exited", pid_str);
                        return;
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
        log::warn!("Timed out waiting for parent process {}", pid_str);
    }

    #[cfg(not(target_os = "windows"))]
    {
        // On Linux/Mac, check /proc/{pid} or use kill(0)
        use std::path::Path;
        for _ in 0..30 {
            let proc_path = format!("/proc/{}", pid_str);
            if !Path::new(&proc_path).exists() {
                log::info!("Parent process {} has exited", pid_str);
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        log::warn!("Timed out waiting for parent process {}", pid_str);
    }
}
