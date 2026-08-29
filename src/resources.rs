//! Process resource limits and monitoring
//!
//! Neolink is long lived and holds a file descriptor per camera connection,
//! per RTSP client and per outgoing HTTP request. Containers frequently start
//! processes with a low `RLIMIT_NOFILE` soft limit (1024 is still common) even
//! though the hard limit is orders of magnitude higher, which leaves very
//! little headroom. Running out is unrecoverable in practice: every reconnect
//! needs a descriptor, so the process spins forever logging
//! `Too many open files` until it is restarted.
//!
//! This module raises the soft limit to the hard limit at startup and keeps an
//! eye on descriptor usage so a leak is visible in the log long before it
//! becomes fatal.

use log::*;

/// Raise the open file descriptor soft limit to the hard limit.
///
/// Returns the (soft, hard) limit in effect afterwards, when known.
pub(crate) fn raise_fd_limit() -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        // SAFETY: `getrlimit`/`setrlimit` only read/write the rlimit struct we
        // provide and report failure through the return value.
        unsafe {
            let mut lim = std::mem::MaybeUninit::<libc::rlimit>::zeroed().assume_init();
            if libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim) != 0 {
                return None;
            }
            if lim.rlim_cur < lim.rlim_max {
                let wanted = libc::rlimit {
                    rlim_cur: lim.rlim_max,
                    rlim_max: lim.rlim_max,
                };
                if libc::setrlimit(libc::RLIMIT_NOFILE, &wanted) == 0 {
                    lim = wanted;
                } else {
                    warn!(
                        "Could not raise the open file limit from {} to {}",
                        lim.rlim_cur, lim.rlim_max
                    );
                }
            }
            Some((lim.rlim_cur as u64, lim.rlim_max as u64))
        }
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// The current open file descriptor soft limit, when known
pub(crate) fn fd_limit() -> Option<u64> {
    #[cfg(unix)]
    {
        // SAFETY: `getrlimit` only writes into the struct we provide.
        unsafe {
            let mut lim = std::mem::MaybeUninit::<libc::rlimit>::zeroed().assume_init();
            if libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim) != 0 {
                return None;
            }
            Some(lim.rlim_cur as u64)
        }
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// The number of file descriptors this process currently has open, when knowable
pub(crate) fn open_fd_count() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        // The read_dir handle itself is one of the entries; close enough for
        // monitoring purposes.
        std::fs::read_dir("/proc/self/fd")
            .ok()
            .map(|entries| entries.count() as u64)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Watch file descriptor usage and complain before it becomes fatal.
///
/// Descriptor exhaustion is the failure mode that takes the whole process down
/// without any single component reporting an error, so this logs the trend
/// rather than waiting for the first `Too many open files`.
pub(crate) fn spawn_fd_monitor() {
    if open_fd_count().is_none() {
        // Not observable on this platform
        return;
    }
    tokio::task::spawn(async move {
        let mut warned_at = 0u8;
        let mut peak = 0u64;
        let mut ticks = 0u64;
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let (Some(open), Some(limit)) = (open_fd_count(), fd_limit()) else {
                return;
            };
            if limit == 0 {
                continue;
            }
            if open > peak {
                peak = open;
            }
            let pct = ((open as f64 / limit as f64) * 100.0) as u8;
            ticks += 1;
            // Hourly at info so a slow leak is visible in a normal log without
            // having to enable debug logging or exec into the container. The
            // breakdown by type is always included so the log identifies WHAT
            // is leaking, not just that something is — a leak can be obvious
            // long before it is a large percentage of the limit.
            if ticks % 60 == 0 {
                let types = fd_breakdown().unwrap_or_default();
                info!("Open file descriptors: {open} of {limit} ({pct}%), peak {peak} [{types}]");
            } else {
                debug!("Open file descriptors: {open} of {limit} ({pct}%), peak {peak}");
            }
            // Warn once per threshold crossed rather than every minute
            for threshold in [50u8, 75, 90] {
                if pct >= threshold && warned_at < threshold {
                    warned_at = threshold;
                    let types = fd_breakdown().unwrap_or_default();
                    warn!(
                        "Open file descriptors at {pct}% of the limit ({open} of {limit}) [{types}]. \
                         If this keeps climbing the process will start failing with \
                         `Too many open files`; please report it with your config."
                    );
                }
            }
            if pct < 40 {
                warned_at = 0;
            }
        }
    });
}

/// Whether this error was caused by running out of file descriptors.
///
/// This is worth calling out explicitly because it presents as an ordinary
/// connection failure everywhere in the app, while actually being a
/// process-wide condition that no amount of reconnecting can fix.
pub(crate) fn is_fd_exhaustion(err: &anyhow::Error) -> bool {
    #[cfg(unix)]
    {
        err.chain().any(|cause| {
            cause
                .downcast_ref::<std::io::Error>()
                .and_then(|io| io.raw_os_error())
                .is_some_and(|code| code == libc::EMFILE || code == libc::ENFILE)
        })
    }
    #[cfg(not(unix))]
    {
        let _ = err;
        false
    }
}

/// A short human readable description of current descriptor usage
pub(crate) fn fd_usage_summary() -> String {
    match (open_fd_count(), fd_limit()) {
        (Some(open), Some(limit)) => format!("{open} open of a limit of {limit}"),
        (None, Some(limit)) => format!("limit is {limit}"),
        _ => "unknown".to_string(),
    }
}

/// Classify this process's open descriptors by type, when knowable.
///
/// This is what turns "we ran out of descriptors" into "we know what we ran
/// out of": sockets point at connection handling, pipes at GStreamer/GLib
/// internals (GstPoll/GWakeup socketpairs), anon inodes at event loops.
pub(crate) fn fd_breakdown() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let entries = std::fs::read_dir("/proc/self/fd").ok()?;
        let mut sockets = 0u64;
        let mut pipes = 0u64;
        let mut anon = 0u64;
        let mut files = 0u64;
        let mut other = 0u64;
        for entry in entries.flatten() {
            let Ok(target) = std::fs::read_link(entry.path()) else {
                other += 1;
                continue;
            };
            let target = target.to_string_lossy().into_owned();
            if target.starts_with("socket:") {
                sockets += 1;
            } else if target.starts_with("pipe:") {
                pipes += 1;
            } else if target.starts_with("anon_inode:") {
                anon += 1;
            } else if target.starts_with('/') {
                files += 1;
            } else {
                other += 1;
            }
        }
        Some(format!(
            "sockets: {sockets}, pipes: {pipes}, anon: {anon}, files: {files}, other: {other}"
        ))
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}
