//! What the machine has to spare right now: free memory, read through
//! `sysinfo`. It can be unknown (the system refused to tell): then the
//! checks that use it are skipped rather than blocking the work, and the
//! log says why.

/// Memory the system can give right now, in bytes; `None` if it cannot be
/// told.
pub fn available_memory() -> Option<u64> {
    let mut sys = match sysinfo::System::new() {
        Ok(sys) => sys,
        Err(e) => {
            tracing::warn!(error = %e, "free memory unknown: the memory checks are skipped");
            return None;
        }
    };
    sys.refresh_memory();
    let available = sys.available_memory();
    if available == 0 {
        tracing::warn!("free memory reported as 0: the memory checks are skipped");
        return None;
    }
    Some(available)
}
