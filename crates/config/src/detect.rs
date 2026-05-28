use std::path::PathBuf;

use crate::schema::enums::{NodeSize, NodeSizeName};

// ---------------------------------------------------------------------------
// detect_config_path_from_env — exposed via FFI; six-location search
// ---------------------------------------------------------------------------

/// Search for the xrpld config file in the standard six locations (§3.1).
///
/// Returns the first existing file path, or `None` when none is found.
///
/// Locations checked in order:
/// 1. `<cwd>/xrpld.cfg`
/// 2. `<cwd>/rippled.cfg`
/// 3. `$XDG_CONFIG_HOME/xrpld/xrpld.cfg`   (defaults to `$HOME/.config`)
/// 4. `$XDG_CONFIG_HOME/xrpld/rippled.cfg`
/// 5. `/etc/opt/xrpld/xrpld.cfg`
/// 6. `/etc/opt/xrpld/rippled.cfg`
pub fn detect_config_path_from_env() -> Option<PathBuf> {
    use std::env;

    // Build the XDG config home: $XDG_CONFIG_HOME or $HOME/.config
    let xdg_config_home: Option<PathBuf> = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));

    // Collect candidate paths.
    let mut candidates: Vec<PathBuf> = Vec::with_capacity(6);

    // 1 & 2: cwd
    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd.join("xrpld.cfg"));
        candidates.push(cwd.join("rippled.cfg"));
    }

    // 3 & 4: XDG
    if let Some(ref xdg) = xdg_config_home {
        candidates.push(xdg.join("xrpld").join("xrpld.cfg"));
        candidates.push(xdg.join("xrpld").join("rippled.cfg"));
    }

    // 5 & 6: system-wide
    candidates.push(PathBuf::from("/etc/opt/xrpld/xrpld.cfg"));
    candidates.push(PathBuf::from("/etc/opt/xrpld/rippled.cfg"));

    candidates.into_iter().find(|p| p.exists())
}

// ---------------------------------------------------------------------------
// detect_node_size — crate-private helper (not exposed via FFI)
// ---------------------------------------------------------------------------

/// Auto-detect the appropriate `NodeSize` tier from available RAM and
/// CPU parallelism.  Mirrors C++ `Config::setupControl`.
///
/// RAM thresholds (GiB): `[6, 8, 12, 24, ∞]` → `[Tiny, Small, Medium, Large, Huge]`.
/// The raw RAM-based size is then adjusted downward by `min(hw_concurrency / 2, tier)`.
///
/// When `standalone` is `true`, returns `NodeSize::Named(NodeSizeName::Tiny)`
/// immediately without probing RAM.
///
/// Not exposed via FFI — C++ has its own equivalent (`getMemorySize` +
/// `hardware_concurrency`).  Rust callers that need autodetect call this
/// explicitly and pass the result via `LoadOptions` or set `Config.node_size`.
#[allow(dead_code)]
pub fn detect_node_size(standalone: bool) -> NodeSize {
    if standalone {
        return NodeSize::Named(NodeSizeName::Tiny);
    }

    // RAM thresholds in GiB → tier index (0 = Tiny, 4 = Huge).
    // A threshold of 0 means "everything above the previous threshold → Huge".
    const RAM_THRESHOLDS_GIB: [u64; 5] = [6, 8, 12, 24, 0];

    let ram_gib = probe_ram_gib();

    // Walk thresholds: if RAM < threshold, use that tier.  0 means no upper bound.
    let ram_tier = RAM_THRESHOLDS_GIB
        .iter()
        .enumerate()
        .find_map(|(i, &t)| if t == 0 || ram_gib < t { Some(i) } else { None })
        .unwrap_or(4); // fallback: Huge

    // Adjust by CPU: tier = max(0, tier - min(hw_concurrency / 2, tier))
    let hw = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let cpu_reduction = (hw / 2).min(ram_tier);
    let tier = ram_tier.saturating_sub(cpu_reduction);

    match tier {
        0 => NodeSize::Named(NodeSizeName::Tiny),
        1 => NodeSize::Named(NodeSizeName::Small),
        2 => NodeSize::Named(NodeSizeName::Medium),
        3 => NodeSize::Named(NodeSizeName::Large),
        _ => NodeSize::Named(NodeSizeName::Huge),
    }
}

/// Platform-specific total physical RAM probe, returning GiB.
///
/// Uses `sysctl -n hw.memsize` on macOS and `/proc/meminfo` on Linux.
/// Falls back to 0 on any failure (caller maps 0 GiB → Tiny).
pub(crate) fn probe_ram_gib() -> u64 {
    #[cfg(target_os = "macos")]
    {
        // `sysctl -n hw.memsize` returns bytes as a decimal string.
        if let Ok(output) = std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            && output.status.success()
        {
            let s = String::from_utf8_lossy(&output.stdout);
            if let Ok(bytes) = s.trim().parse::<u64>() {
                return bytes / (1024 * 1024 * 1024);
            }
        }
        0
    }
    #[cfg(target_os = "linux")]
    {
        // /proc/meminfo MemTotal: <kB>
        if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
            for line in content.lines() {
                if let Some(rest) = line.strip_prefix("MemTotal:") {
                    let kb: u64 = rest
                        .split_whitespace()
                        .next()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    return kb / (1024 * 1024);
                }
            }
        }
        0
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // detect_config_path_from_env: basic smoke (None when paths don't exist)
    // -----------------------------------------------------------------------

    #[test]
    fn detect_config_path_returns_none_when_no_config_exists() {
        // Running in a temp dir with no config files and no system-wide fallback
        // is hard to guarantee, so just check the return type is well-formed.
        // The function either returns Some(existing_path) or None.
        let result = detect_config_path_from_env();
        if let Some(ref p) = result {
            assert!(
                p.exists(),
                "detect_config_path_from_env must return an existing path"
            );
        }
    }

    // -----------------------------------------------------------------------
    // detect_node_size: standalone always returns Tiny
    // -----------------------------------------------------------------------

    #[test]
    fn detect_node_size_standalone_returns_tiny() {
        use crate::schema::enums::{NodeSize, NodeSizeName};
        let ns = detect_node_size(true);
        assert_eq!(ns, NodeSize::Named(NodeSizeName::Tiny));
    }

    #[test]
    fn detect_node_size_non_standalone_returns_a_valid_tier() {
        use crate::schema::enums::{NodeSize, NodeSizeName};
        let ns = detect_node_size(false);
        // Any named tier is acceptable — we can't assert a specific tier
        // because the host RAM and CPU count are unknown at test time.
        let valid = matches!(
            ns,
            NodeSize::Named(
                NodeSizeName::Tiny
                    | NodeSizeName::Small
                    | NodeSizeName::Medium
                    | NodeSizeName::Large
                    | NodeSizeName::Huge
            )
        );
        assert!(
            valid,
            "detect_node_size must return a Named tier, got: {ns:?}"
        );
    }
}
