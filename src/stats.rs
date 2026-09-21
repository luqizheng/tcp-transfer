use std::sync::atomic::{AtomicU64, Ordering};

/// Process-wide counters, lock-free.
#[derive(Default)]
pub struct Stats {
    pub total_conns: AtomicU64,
    pub active_conns: AtomicU64,
    /// Bytes from listener side (client) -> target.
    pub bytes_in: AtomicU64,
    /// Bytes from target -> listener side (client).
    pub bytes_out: AtomicU64,
}

impl Stats {
    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            total_conns: self.total_conns.load(Ordering::Relaxed),
            active_conns: self.active_conns.load(Ordering::Relaxed),
            bytes_in: self.bytes_in.load(Ordering::Relaxed),
            bytes_out: self.bytes_out.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StatsSnapshot {
    pub total_conns: u64,
    pub active_conns: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

impl StatsSnapshot {
    pub fn human_bytes(n: u64) -> String {
        const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
        let mut v = n as f64;
        let mut i = 0;
        while v >= 1024.0 && i < UNITS.len() - 1 {
            v /= 1024.0;
            i += 1;
        }
        format!("{:.2} {}", v, UNITS[i])
    }
}
