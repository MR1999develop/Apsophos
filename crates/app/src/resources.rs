//! What the machine has to spare right now: free memory and free disk
//! space, read through `sysinfo`. Either can be unknown (the system refused
//! to tell): then the checks that use it are skipped rather than blocking
//! the work, and the log says why.

use std::path::Path;

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

/// Free space on the volume holding `dir`, in bytes; `None` if it cannot be
/// told.
pub fn free_space(dir: &Path) -> Option<u64> {
    let dir = std::path::absolute(dir).ok()?;
    let disks = match sysinfo::Disks::new_with_refreshed_list() {
        Ok(disks) => disks,
        Err(e) => {
            tracing::warn!(error = %e, "free disk space unknown: the space check is skipped");
            return None;
        }
    };
    let free = disks
        .iter()
        .filter(|d| dir.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(sysinfo::Disk::available_space);
    if free.is_none() {
        tracing::warn!(folder = %dir.display(), "no volume found for the folder: the space check is skipped");
    }
    free
}
