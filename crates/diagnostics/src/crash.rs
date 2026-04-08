use crate::reporter::LogBuffer;
use std::backtrace::Backtrace;
use std::path::Path;

/// Install a panic hook that writes crash files and logs via tracing.
pub fn install_panic_hook(log_dir: &Path, log_buffer: LogBuffer, app_version: &str) {
    // Force full backtraces with resolved symbols.  Must be set BEFORE any
    // panic fires so Backtrace::force_capture() picks it up.  `line-tables-only`
    // debug info is already on in release profile — this just tells the runtime
    // to resolve symbols to names rather than emitting `<unknown>` frames.
    // SAFETY: set_var is unsafe in edition 2024; here we're on 2021 where it
    // is still safe, and we call it once at startup before threads spawn.
    std::env::set_var("RUST_BACKTRACE", "full");
    std::env::set_var("RUST_LIB_BACKTRACE", "full");

    let log_dir = log_dir.to_owned();
    let version = app_version.to_string();

    std::panic::set_hook(Box::new(move |info| {
        // Capture backtrace FIRST
        let bt = Backtrace::force_capture();

        // Extract location
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".to_string());

        // Extract message
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic payload".to_string()
        };

        // Thread context — which thread panicked?
        let thread = std::thread::current();
        let thread_name = thread.name().unwrap_or("<unnamed>").to_string();
        let thread_id = format!("{:?}", thread.id());

        // Log via tracing (best effort)
        tracing::error!(
            panic.location = %location,
            panic.message = %msg,
            panic.thread = %thread_name,
            "PANIC"
        );

        // Get recent log lines — 200 is enough to catch the event sequence
        // that led to the panic (resize, drag, input routing, etc.).
        let recent_logs = log_buffer.last_n(200);
        let recent_logs_text = recent_logs.join("\n");

        // Sanitize only the location string (keep backtrace raw so function
        // names survive — symbols are what we need for diagnosis).
        let bt_text = format!("{bt}");
        let location_sanitized = sanitize_paths(&location);

        // Build crash report
        let timestamp = chrono::Local::now();
        let crash_report = format!(
            "=== CRASH REPORT ===\n\
             timestamp: {}\n\
             version: {}\n\
             pid: {}\n\
             thread: {} ({})\n\
             location: {}\n\
             message: {}\n\
             \n\
             === BACKTRACE ===\n\
             {}\n\
             \n\
             === RECENT LOGS (last 200) ===\n\
             {}\n",
            timestamp.to_rfc3339(),
            version,
            std::process::id(),
            thread_name,
            thread_id,
            location_sanitized,
            msg,
            bt_text,
            recent_logs_text,
        );

        // Write crash file
        let crash_file = log_dir.join(format!("crash-{}.txt", timestamp.format("%Y%m%d-%H%M%S")));
        let _ = std::fs::write(&crash_file, &crash_report);

        // Also print to stderr
        eprintln!("{crash_report}");
    }));
}

/// Strip Windows usernames from paths for privacy.
fn sanitize_paths(text: &str) -> String {
    let mut result = text.to_string();

    // Handle backslash paths: C:\Users\<username>\
    if let Some(start) = result.find("C:\\Users\\") {
        if let Some(end) = result[start + 9..].find('\\') {
            let username = result[start..start + 9 + end + 1].to_string();
            result = result.replace(&username, "<home>\\");
        }
    }

    // Handle forward-slash paths: C:/Users/<username>/
    if let Some(start) = result.find("C:/Users/") {
        if let Some(end) = result[start + 9..].find('/') {
            let username = result[start..start + 9 + end + 1].to_string();
            result = result.replace(&username, "<home>/");
        }
    }

    result
}
