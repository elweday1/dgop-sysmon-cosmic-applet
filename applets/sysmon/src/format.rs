// SPDX-License-Identifier: GPL-3.0-only

pub fn rate(bytes_per_sec: f64) -> String {
    const UNITS: [&str; 4] = ["B/s", "KB/s", "MB/s", "GB/s"];
    let mut value = bytes_per_sec.max(0.0);
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

pub fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

pub fn memory_short(kb: u64) -> String {
    format!("{:.1}G", kb as f64 / (1024.0 * 1024.0))
}

pub fn uptime() -> String {
    let secs = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
        .unwrap_or(0.0) as u64;
    let (days, hours, minutes) = (secs / 86400, (secs % 86400) / 3600, (secs % 3600) / 60);
    if days > 0 {
        format!("up {days}d {hours}h {minutes}m")
    } else if hours > 0 {
        format!("up {hours}h {minutes}m")
    } else {
        format!("up {minutes}m")
    }
}

pub fn memory_kb(kb: u64) -> String {
    if kb < 1024 * 1024 {
        format!("{:.0} MB", kb as f64 / 1024.0)
    } else {
        format!("{:.1} GB", kb as f64 / (1024.0 * 1024.0))
    }
}
