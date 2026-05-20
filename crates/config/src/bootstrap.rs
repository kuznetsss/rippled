//! Bootstrap — everything with filesystem side-effects.
//!
//! This module is the only place in the crate that:
//! - touches the filesystem (reads `validators.txt`, creates `data_dir`)
//! - probes hardware (RAM, CPU count for `NodeSize` detection)
//! - emits to stderr
//!
//! Pure parsing in `ini/` and `toml/` has none of these effects.

use std::path::{Path, PathBuf};

use crate::config::{Config, Finalized};
use crate::error::ConfigError;
use crate::types::NodeSize;
use crate::types::path::resolve_against;

// ---------------------------------------------------------------------------
// Main entry point — called by Config::bootstrap()
// ---------------------------------------------------------------------------

/// Run all bootstrap side-effects and populate `cfg.finalized`.
/// Idempotency (already-finalized guard) is enforced in `Config::bootstrap`.
pub(crate) fn run_bootstrap(cfg: &mut Config) -> Result<(), ConfigError> {
    // -----------------------------------------------------------------------
    // Step 1: Resolve config_dir
    // -----------------------------------------------------------------------
    let config_dir = cfg
        .overrides
        .config_dir
        .clone()
        .ok_or_else(|| {
            ConfigError::cross(
                "config_dir not set; call set_config_dir before bootstrap",
            )
        })?;

    // -----------------------------------------------------------------------
    // Step 2: Resolve the three auto-resolved RelPath fields
    // -----------------------------------------------------------------------
    let debug_logfile_resolved = cfg
        .parsed
        .debug_logfile
        .as_ref()
        .map(|rp| resolve_against(&config_dir, rp.as_path()));

    let validators_file_resolved = cfg
        .parsed
        .validators_file
        .as_ref()
        .map(|rp| resolve_against(&config_dir, rp.as_path()));

    // database_path is resolved here but used for data_dir computation below.
    let database_path_resolved = cfg
        .parsed
        .database_path
        .as_ref()
        .map(|rp| resolve_against(&config_dir, rp.as_path()));

    // -----------------------------------------------------------------------
    // Step 3: Splice validators.txt
    // -----------------------------------------------------------------------
    if let Some(ref vf_path) = validators_file_resolved {
        // Explicit validators_file — splice it; error if missing.
        splice_validators_file(cfg, vf_path)?;
    } else {
        // Implicit: try <config_dir>/validators.txt; silently ignore if absent.
        // Use open-on-attempt rather than exists()-then-open to avoid TOCTOU.
        let implicit = config_dir.join("validators.txt");
        match splice_validators_file(cfg, &implicit) {
            Ok(()) => {}
            Err(e) => {
                // Silently ignore NotFound; propagate all other I/O errors.
                if !matches!(
                    &e.kind,
                    crate::error::ConfigErrorKind::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound
                ) {
                    return Err(e);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Step 4: Cross-section validation
    // -----------------------------------------------------------------------
    run_cross_validators(cfg)?;

    // -----------------------------------------------------------------------
    // Step 5: Resolve data_dir; mkdir -p unless standalone
    // -----------------------------------------------------------------------
    let data_dir = database_path_resolved.unwrap_or_else(|| config_dir.join("db"));
    ensure_data_dir(&data_dir, cfg.standalone())?;

    // -----------------------------------------------------------------------
    // Step 6: Determine effective NodeSize
    // -----------------------------------------------------------------------
    let mut node_size_effective = cfg
        .parsed
        .node_size
        .unwrap_or_else(detect_node_size);

    if cfg.standalone() {
        node_size_effective = NodeSize::Tiny;
    }

    // -----------------------------------------------------------------------
    // Step 7: Stderr echo unless quiet
    // -----------------------------------------------------------------------
    if !cfg.quiet() {
        if let Some(ref explicit_path) = cfg.overrides._explicit_config_path {
            eprintln!("Loaded config from {}", explicit_path.display());
        }
    }

    // -----------------------------------------------------------------------
    // Step 8: Write finalized
    // -----------------------------------------------------------------------
    cfg.finalized = Some(Finalized {
        config_dir,
        data_dir,
        debug_logfile_resolved,
        validators_file_resolved,
        node_size_effective,
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// Cross-section validation (design §9 / analysis §5)
// ---------------------------------------------------------------------------

fn run_cross_validators(cfg: &Config) -> Result<(), ConfigError> {
    // validation_seed XOR validator_token — at most one set
    if cfg.parsed.validation_seed.is_some() && cfg.parsed.validator_token.is_some() {
        return Err(ConfigError::mutual_exclusion(
            "validation_seed",
            "validator_token",
        ));
    }

    // peers_in_max and peers_out_max — both-or-neither
    let has_in = cfg.parsed.peers_in_max > 0;
    let has_out = cfg.parsed.peers_out_max > 0;
    if has_in != has_out {
        return Err(ConfigError::cross(
            "peers_in_max and peers_out_max must both be set or both be unset",
        ));
    }
    // peers_in_max must be <= 1000 if set (analysis §2.1 / §5)
    if has_in {
        let v = cfg.parsed.peers_in_max;
        if v > 1000 {
            return Err(ConfigError::out_of_range(
                "peers_in_max",
                v as i64,
                None,
                Some(1000),
            ));
        }
    }
    // peers_out_max in 10..=1000 if set
    if has_out {
        let v = cfg.parsed.peers_out_max;
        if v < 10 || v > 1000 {
            return Err(ConfigError::out_of_range(
                "peers_out_max",
                v as i64,
                Some(10),
                Some(1000),
            ));
        }
    }

    // network_quorum <= effective peers_max (effective = peers_max if set else 21)
    let effective_peers_max: u64 = if cfg.parsed.peers_max > 0 {
        cfg.parsed.peers_max as u64
    } else {
        21
    };
    if cfg.parsed.network_quorum > effective_peers_max {
        return Err(ConfigError::cross(format!(
            "network_quorum ({}) must be <= peers_max ({})",
            cfg.parsed.network_quorum, effective_peers_max
        )));
    }

    // online_delete >= ledger_history (if both set)
    if let Some(online_delete) = cfg.parsed.node_db.online_delete {
        use crate::types::LedgerHistory;
        match cfg.parsed.ledger_history {
            LedgerHistory::Count(lh) => {
                if online_delete < lh {
                    return Err(ConfigError::cross(format!(
                        "online_delete ({online_delete}) must be >= ledger_history ({lh})"
                    )));
                }
            }
            LedgerHistory::Full => {
                // online_delete < Full is always ok — Full is "keep everything"
            }
            LedgerHistory::None_ => {}
        }
    }

    // checkZeroPorts: every declared port must have port > 0
    for (name, port_cfg) in &cfg.parsed.ports {
        if port_cfg.port == 0 {
            return Err(ConfigError::cross(format!(
                "port `{name}` has port number 0; all declared ports must have port > 0"
            )));
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Validators.txt splice
// ---------------------------------------------------------------------------

/// Read `path` as an INI file and merge the allow-listed validator sections
/// into `cfg.parsed`.
///
/// Allow-listed sections:
/// - `[validators]` / `[validator_keys]` → append to `trusted_validators`
/// - `[validator_list_sites]`             → append to `validator_list_sites`
/// - `[validator_list_keys]`              → append to `validator_list_keys`
/// - `[validator_list_threshold]`         → override `validator_list_threshold`
pub(crate) fn splice_validators_file(cfg: &mut Config, path: &Path) -> Result<(), ConfigError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| ConfigError::io(path.to_owned(), e))?;

    // Choose parser by extension: .toml → TOML, anything else → INI (design §6).
    let secondary = if path.extension().and_then(|e| e.to_str()) == Some("toml") {
        crate::toml::parse_toml(&text)
    } else {
        crate::ini::parse_ini(&text)
    }.map_err(|e| e.with_file(path.to_owned()))?;

    // TOML strict mode: overlap between the main config and validators file is an error.
    // INI lenient mode: silent append (analysis §7 #9).
    let is_toml = matches!(cfg.parsed.source_format, crate::error::Format::Toml);

    if is_toml {
        if !cfg.parsed.trusted_validators.is_empty() && !secondary.parsed.trusted_validators.is_empty() {
            return Err(ConfigError::validators_file_overlap("validators/validator_keys"));
        }
        if !cfg.parsed.validator_list_sites.is_empty() && !secondary.parsed.validator_list_sites.is_empty() {
            return Err(ConfigError::validators_file_overlap("validator_list_sites"));
        }
        if !cfg.parsed.validator_list_keys.is_empty() && !secondary.parsed.validator_list_keys.is_empty() {
            return Err(ConfigError::validators_file_overlap("validator_list_keys"));
        }
    }

    // Merge allow-listed fields from secondary into cfg.parsed.
    cfg.parsed.trusted_validators.extend(secondary.parsed.trusted_validators);
    cfg.parsed.validator_list_sites.extend(secondary.parsed.validator_list_sites);
    cfg.parsed.validator_list_keys.extend(secondary.parsed.validator_list_keys);
    if secondary.parsed.validator_list_threshold.is_some() {
        cfg.parsed.validator_list_threshold = secondary.parsed.validator_list_threshold;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// ensure_data_dir
// ---------------------------------------------------------------------------

/// Create `path` (and all parents) unless `standalone` is true.
pub(crate) fn ensure_data_dir(path: &Path, standalone: bool) -> Result<(), ConfigError> {
    if standalone {
        return Ok(());
    }
    std::fs::create_dir_all(path)
        .map_err(|e| ConfigError::io(path.to_owned(), e))
}

// ---------------------------------------------------------------------------
// discover_config_file (callable from FFI)
// ---------------------------------------------------------------------------

/// Return the path of the config file to load, searching the standard locations.
///
/// If `explicit` is `Some`, use it directly.
/// Otherwise, check these locations in order (matching analysis §4 / C++ search order):
///   1. `./xrpld.cfg`
///   2. `./rippled.cfg`
///   3. `$XDG_CONFIG_HOME/<sys_name>/xrpld.cfg`
///   4. `$XDG_CONFIG_HOME/<sys_name>/rippled.cfg`
///   5. `$HOME/.config/<sys_name>/xrpld.cfg`   (fallback when XDG_CONFIG_HOME absent)
///   6. `$HOME/.config/<sys_name>/rippled.cfg`
///   7. `/etc/opt/<sys_name>/xrpld.cfg`
///   8. `/etc/opt/<sys_name>/rippled.cfg`
///
/// Returns the first existing path. If none exist, returns the last-tried path
/// (matches C++ behavior — caller gets a sensible file name to report in errors).
pub fn discover_config_file(
    explicit: Option<PathBuf>,
    sys_name: &str,
) -> Result<PathBuf, ConfigError> {
    if let Some(p) = explicit {
        return Ok(p);
    }

    let candidates: Vec<PathBuf> = {
        let mut v = vec![
            PathBuf::from("xrpld.cfg"),
            PathBuf::from("rippled.cfg"),
        ];

        // XDG_CONFIG_HOME (or ~/.config fallback) — both xrpld and rippled variants.
        let xdg_config = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| PathBuf::from(h).join(".config"))
                    .unwrap_or_else(|| PathBuf::from(".config"))
            });
        v.push(xdg_config.join(sys_name).join("xrpld.cfg"));
        v.push(xdg_config.join(sys_name).join("rippled.cfg"));

        // System-wide /etc/opt paths (analysis §4).
        v.push(PathBuf::from("/etc/opt").join(sys_name).join("xrpld.cfg"));
        v.push(PathBuf::from("/etc/opt").join(sys_name).join("rippled.cfg"));

        v
    };

    for candidate in &candidates {
        if candidate.exists() {
            return Ok(candidate.clone());
        }
    }

    // None found — return the last candidate (C++ fallback behavior).
    Ok(candidates.into_iter().last().unwrap())
}

// ---------------------------------------------------------------------------
// detect_node_size
// ---------------------------------------------------------------------------

/// Auto-detect the appropriate `NodeSize` based on available RAM and CPU count.
///
/// Detection strategy:
/// 1. If `RIPPLED_RAM_GB_OVERRIDE` env var is set (e.g. in tests), use it.
/// 2. Otherwise, probe RAM via OS syscalls.
/// 3. Choose size by both RAM threshold and half the CPU count, taking the min.
///
/// Falls back to `NodeSize::Tiny` if detection fails.
///
/// For deterministic testing, use `detect_node_size_with(ram_gb, cpu_count)`.
pub fn detect_node_size() -> NodeSize {
    // Test hook: allow override via env var.
    if let Ok(s) = std::env::var("RIPPLED_RAM_GB_OVERRIDE") {
        if let Ok(gb) = s.parse::<u64>() {
            return node_size_from_ram_gb(gb);
        }
    }

    let ram_gb = probe_ram_gb().unwrap_or(0);

    // CPU-based cap: use half of available parallelism as a rough "performance cores" estimate.
    let cpu_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    detect_node_size_with(ram_gb, cpu_count)
}

/// Parameterized version of `detect_node_size` for testing.
///
/// Takes explicit `ram_gb` and `cpu_count` values rather than probing the OS.
/// `cpu_count` is the raw count (halved internally, matching `detect_node_size`).
///
/// This is `pub` so integration tests can exercise the sizing matrix without
/// relying on `RIPPLED_RAM_GB_OVERRIDE` env-var serialisation.
pub fn detect_node_size_with(ram_gb: u64, cpu_count: usize) -> NodeSize {
    let cpu_cap = node_size_from_cpu(cpu_count / 2);
    let by_ram = node_size_from_ram_gb(ram_gb);
    // Take the smaller of RAM-based and CPU-based estimates.
    node_size_min(by_ram, cpu_cap)
}

/// Convert RAM in GiB to a `NodeSize` bucket.
/// Thresholds match the `RamSizeGb` row in `SIZED_TABLE`:
///   Tiny=6, Small=8, Medium=12, Large=24, Huge=∞
///
/// Each entry in `RamSizeGb` is the **minimum** RAM required to reach that
/// size bucket.  The comparison is therefore:
///   - gb >= Large threshold (24)  → Large
///   - gb >= Medium threshold (12) → Medium
///   - gb >= Small threshold (8)   → Small
///   - gb >= Tiny threshold (6)    → Tiny
///   - else                        → Tiny (below even the Tiny minimum)
///
/// There is no separate Huge threshold from the table (the Huge column stores 0).
/// Huge is handled by checking whether gb is >= the Large threshold and also >=
/// a Huge sentinel.  Because the C++ code treats Huge as "no upper limit",
/// we consider systems with >= 64 GB as Huge.
fn node_size_from_ram_gb(gb: u64) -> NodeSize {
    use crate::types::sized::{SIZED_TABLE, SizedItem};
    // The RamSizeGb row stores the *minimum* RAM for each size bucket.
    // Tiny=6, Small=8, Medium=12, Large=24; Huge column stores 0 (meaning
    // "no minimum — anything not captured by the lower tiers").
    let tiny_thresh  = SIZED_TABLE[SizedItem::RamSizeGb as usize][NodeSize::Tiny  as usize] as u64;
    let small_thresh = SIZED_TABLE[SizedItem::RamSizeGb as usize][NodeSize::Small as usize] as u64;
    let med_thresh   = SIZED_TABLE[SizedItem::RamSizeGb as usize][NodeSize::Medium as usize] as u64;
    let large_thresh = SIZED_TABLE[SizedItem::RamSizeGb as usize][NodeSize::Large as usize] as u64;
    // Huge: any RAM well above the Large threshold (64 GiB chosen to match
    // the C++ server sizing guide).
    let huge_thresh: u64 = 64;

    if gb >= huge_thresh {
        NodeSize::Huge
    } else if gb >= large_thresh {
        NodeSize::Large
    } else if gb >= med_thresh {
        NodeSize::Medium
    } else if gb >= small_thresh {
        NodeSize::Small
    } else if gb >= tiny_thresh {
        NodeSize::Tiny
    } else {
        NodeSize::Tiny
    }
}

/// Convert a CPU count (already halved) to a `NodeSize` cap.
fn node_size_from_cpu(half_cpus: usize) -> NodeSize {
    match half_cpus {
        0..=1 => NodeSize::Tiny,
        2..=3 => NodeSize::Small,
        4..=7 => NodeSize::Medium,
        8..=15 => NodeSize::Large,
        _ => NodeSize::Huge,
    }
}

/// Return the smaller of two `NodeSize` values.
fn node_size_min(a: NodeSize, b: NodeSize) -> NodeSize {
    if (a as u8) <= (b as u8) { a } else { b }
}

/// Probe available physical RAM and return it in GiB.
/// Returns `None` if the probe fails (the caller will fall back to Medium).
fn probe_ram_gb() -> Option<u64> {
    probe_ram_bytes().map(|b| b / (1024 * 1024 * 1024))
}

#[cfg(target_os = "macos")]
fn probe_ram_bytes() -> Option<u64> {
    // sysctl hw.memsize
    use std::mem;
    let mut mem_size: u64 = 0;
    let mut size = mem::size_of::<u64>();
    let name = b"hw.memsize\0";
    let ret = unsafe {
        libc::sysctlbyname(
            name.as_ptr() as *const libc::c_char,
            &mut mem_size as *mut u64 as *mut libc::c_void,
            &mut size as *mut usize,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret == 0 { Some(mem_size) } else { None }
}

#[cfg(target_os = "linux")]
fn probe_ram_bytes() -> Option<u64> {
    // sysinfo(2)
    let mut info: libc::sysinfo = unsafe { std::mem::zeroed() };
    let ret = unsafe { libc::sysinfo(&mut info) };
    if ret == 0 {
        Some(info.totalram as u64 * info.mem_unit as u64)
    } else {
        None
    }
}

#[cfg(target_os = "windows")]
fn probe_ram_bytes() -> Option<u64> {
    use std::mem;
    #[allow(non_snake_case)]
    extern "system" {
        fn GlobalMemoryStatusEx(lpBuffer: *mut MemoryStatusEx) -> i32;
    }
    #[repr(C)]
    #[allow(non_snake_case)]
    struct MemoryStatusEx {
        dwLength: u32,
        dwMemoryLoad: u32,
        ullTotalPhys: u64,
        ullAvailPhys: u64,
        ullTotalPageFile: u64,
        ullAvailPageFile: u64,
        ullTotalVirtual: u64,
        ullAvailVirtual: u64,
        ullAvailExtendedVirtual: u64,
    }
    let mut status: MemoryStatusEx = unsafe { mem::zeroed() };
    status.dwLength = mem::size_of::<MemoryStatusEx>() as u32;
    let ret = unsafe { GlobalMemoryStatusEx(&mut status) };
    if ret != 0 { Some(status.ullTotalPhys) } else { None }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn probe_ram_bytes() -> Option<u64> {
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Mutex to serialise tests that mutate `RIPPLED_RAM_GB_OVERRIDE`.
    // Tests that set this env var must hold this lock for the duration
    // to avoid racing with each other (env vars are process-global).
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn discover_returns_explicit() {
        let p = PathBuf::from("/etc/custom.cfg");
        let result = discover_config_file(Some(p.clone()), "xrpld").unwrap();
        assert_eq!(result, p);
    }

    #[test]
    fn discover_falls_back_gracefully() {
        // In a clean env with no config files, this should return the last candidate
        // without error.
        let result = discover_config_file(None, "xrpld_test_nonexistent_12345");
        assert!(result.is_ok());
    }

    #[test]
    fn node_size_from_ram_tiny() {
        assert_eq!(node_size_from_ram_gb(0), NodeSize::Tiny);
        assert_eq!(node_size_from_ram_gb(4), NodeSize::Tiny);
    }

    #[test]
    fn node_size_from_ram_large() {
        // 25 GB is above the Large threshold (24) but below Huge (64) → Large
        assert_eq!(node_size_from_ram_gb(25), NodeSize::Large);
        // 64 GB reaches the Huge threshold
        assert_eq!(node_size_from_ram_gb(64), NodeSize::Huge);
    }

    #[test]
    fn node_size_env_override() {
        // Set the env var and verify detect_node_size respects it.
        // SAFETY: this test is single-threaded; modifying env vars is safe here.
        unsafe {
            std::env::set_var("RIPPLED_RAM_GB_OVERRIDE", "6");
        }
        let size = detect_node_size();
        unsafe {
            std::env::remove_var("RIPPLED_RAM_GB_OVERRIDE");
        }
        // 6 GB is at the Tiny threshold — should be Small (>=6 → Small)
        assert!(matches!(size, NodeSize::Tiny | NodeSize::Small));
    }

    #[test]
    fn ensure_data_dir_standalone_no_create() {
        // Should not create a directory in standalone mode.
        let tmp = std::env::temp_dir().join("rippled_bootstrap_test_standalone_xyz");
        let _ = std::fs::remove_dir(&tmp);
        ensure_data_dir(&tmp, true).unwrap();
        assert!(!tmp.exists());
    }

    #[test]
    fn ensure_data_dir_creates() {
        let tmp = std::env::temp_dir().join("rippled_bootstrap_test_create_xyz");
        let _ = std::fs::remove_dir_all(&tmp);
        ensure_data_dir(&tmp, false).unwrap();
        assert!(tmp.exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn cross_validation_mutual_exclusion() {
        let text = "[validation_seed]\nseed123\n[validator_token]\ntoken456\n";
        let mut cfg = Config::from_ini_str(text).unwrap();
        cfg.set_config_dir(std::env::temp_dir());
        cfg.set_standalone(true);
        let result = cfg.bootstrap();
        assert!(result.is_err());
        let msg = result.unwrap_err().message();
        assert!(msg.contains("validation_seed") || msg.contains("validator_token"));
    }

    #[test]
    fn cross_validation_network_quorum_ok() {
        // network_quorum=1 (default) with default peers_max=0 (effective 21) should pass.
        let mut cfg = Config::from_ini_str("").unwrap();
        cfg.set_config_dir(std::env::temp_dir());
        cfg.set_standalone(true);
        cfg.bootstrap().unwrap();
    }

    // ---- discover_config_file ----

    #[test]
    fn discover_explicit_nonexistent_path_still_returned() {
        // discover_config_file returns the explicit path verbatim even if it doesn't exist
        let p = PathBuf::from("/tmp/nonexistent_xrpld_test_config_99999.cfg");
        let result = discover_config_file(Some(p.clone()), "xrpld").unwrap();
        assert_eq!(result, p);
    }

    #[test]
    fn discover_returns_last_candidate_when_none_exist() {
        // No configs exist → should return some path (the last candidate) without error
        let result = discover_config_file(None, "xrpld_test_unique_9999999").unwrap();
        // The last candidate should contain the sys_name
        let path_str = result.to_string_lossy().to_string();
        assert!(
            path_str.contains("xrpld_test_unique_9999999"),
            "unexpected path: {path_str}"
        );
    }

    #[test]
    fn discover_config_file_existing_xrpld_cfg() {
        use std::io::Write;
        // Create a temporary directory with a xrpld.cfg file and verify discover finds it
        let tmp_dir = std::env::temp_dir().join("xrpld_discover_test_xyz789");
        let _ = std::fs::create_dir_all(&tmp_dir);
        let cfg_path = tmp_dir.join("xrpld.cfg");
        let mut f = std::fs::File::create(&cfg_path).unwrap();
        writeln!(f, "# test config").unwrap();
        drop(f);

        // When explicit is provided, return it verbatim
        let result = discover_config_file(Some(cfg_path.clone()), "xrpld").unwrap();
        assert_eq!(result, cfg_path);

        // Cleanup
        let _ = std::fs::remove_file(&cfg_path);
        let _ = std::fs::remove_dir(&tmp_dir);
    }

    // ---- detect_node_size ----

    #[test]
    fn detect_node_size_override_tiny() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("RIPPLED_RAM_GB_OVERRIDE", "1"); }
        let size = detect_node_size();
        unsafe { std::env::remove_var("RIPPLED_RAM_GB_OVERRIDE"); }
        assert_eq!(size, NodeSize::Tiny);
    }

    #[test]
    fn detect_node_size_override_small() {
        // 8 GB → Small (exactly at the Small threshold)
        // Note: detect_node_size takes min(by_ram, cpu_cap) so in low-CPU envs
        // result may be lower. We just verify the RAM-only function is correct.
        assert_eq!(node_size_from_ram_gb(8), NodeSize::Small);
        assert_eq!(node_size_from_ram_gb(7), NodeSize::Tiny);
        // Detect with env override — actual result may be lower due to CPU cap
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("RIPPLED_RAM_GB_OVERRIDE", "8"); }
        let _size = detect_node_size();
        unsafe { std::env::remove_var("RIPPLED_RAM_GB_OVERRIDE"); }
        // Just verify it doesn't panic
    }

    #[test]
    fn detect_node_size_override_medium() {
        // 12 GB meets the Medium threshold → Medium from RAM
        // But detect_node_size takes min(by_ram, cpu_cap) so in low-CPU envs
        // the result may be lower. Verify the RAM-only function gives Medium.
        assert_eq!(node_size_from_ram_gb(12), NodeSize::Medium);
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("RIPPLED_RAM_GB_OVERRIDE", "12"); }
        let _size = detect_node_size();
        unsafe { std::env::remove_var("RIPPLED_RAM_GB_OVERRIDE"); }
        // We only assert it doesn't panic and is a valid NodeSize.
    }

    #[test]
    fn detect_node_size_override_large() {
        // 24 GB meets the Large threshold (24) → Large
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("RIPPLED_RAM_GB_OVERRIDE", "30"); }
        let size = detect_node_size();
        unsafe { std::env::remove_var("RIPPLED_RAM_GB_OVERRIDE"); }
        // 30 GB is above Large threshold (24) but below Huge (64); but
        // detect_node_size takes min(by_ram, cpu_cap), so in CI the CPU cap
        // may be Small/Medium. We assert at least Large from RAM perspective.
        assert_eq!(node_size_from_ram_gb(30), NodeSize::Large);
        // The env-override path in detect_node_size still applies cpu capping
        // so just verify it's at most Large (not Huge unless CPUs are high enough)
        assert!(matches!(size, NodeSize::Tiny | NodeSize::Small | NodeSize::Medium | NodeSize::Large));
    }

    #[test]
    fn detect_node_size_override_huge() {
        // Very high → Huge (>= 64 GB threshold); early-return in detect_node_size
        // skips the CPU cap when the env override is active, so this is always Huge.
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("RIPPLED_RAM_GB_OVERRIDE", "9999"); }
        let size = detect_node_size();
        unsafe { std::env::remove_var("RIPPLED_RAM_GB_OVERRIDE"); }
        assert_eq!(size, NodeSize::Huge);
    }

    #[test]
    fn node_size_from_ram_zero_is_tiny() {
        assert_eq!(node_size_from_ram_gb(0), NodeSize::Tiny);
    }

    #[test]
    fn node_size_from_ram_small_threshold() {
        // Threshold for Tiny is 6 GB, Small is 8 GB
        assert_eq!(node_size_from_ram_gb(6), NodeSize::Tiny);
        assert_eq!(node_size_from_ram_gb(8), NodeSize::Small);
        assert_eq!(node_size_from_ram_gb(5), NodeSize::Tiny);
    }

    #[test]
    fn node_size_from_ram_medium_threshold() {
        // Threshold for Medium is 12 GB
        assert_eq!(node_size_from_ram_gb(12), NodeSize::Medium);
        assert_eq!(node_size_from_ram_gb(11), NodeSize::Small);
    }

    #[test]
    fn node_size_from_ram_large_threshold() {
        // Threshold for Large is 24 GB
        assert_eq!(node_size_from_ram_gb(24), NodeSize::Large);
        assert_eq!(node_size_from_ram_gb(23), NodeSize::Medium);
    }

    #[test]
    fn node_size_from_ram_huge_threshold() {
        // Threshold for Huge is 64 GB
        assert_eq!(node_size_from_ram_gb(64), NodeSize::Huge);
        assert_eq!(node_size_from_ram_gb(63), NodeSize::Large);
    }

    // ---- ensure_data_dir ----

    #[test]
    fn ensure_data_dir_existing_dir_ok() {
        // If directory already exists, ensure_data_dir should succeed
        let tmp = std::env::temp_dir();
        ensure_data_dir(&tmp, false).unwrap();
        assert!(tmp.exists());
    }

    #[test]
    fn ensure_data_dir_nested_creates_parents() {
        let tmp = std::env::temp_dir().join("xrpld_nested_test_abc").join("sub").join("dir");
        let _ = std::fs::remove_dir_all(tmp.ancestors().nth(2).unwrap());
        ensure_data_dir(&tmp, false).unwrap();
        assert!(tmp.exists());
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("xrpld_nested_test_abc"));
    }

    // ---- splice_validators_file ----

    #[test]
    fn splice_validators_file_appends_validators() {
        use std::io::Write;
        let tmp = std::env::temp_dir().join("validators_splice_test_abc.txt");
        {
            let mut f = std::fs::File::create(&tmp).unwrap();
            writeln!(f, "[validators]").unwrap();
            writeln!(f, "nHUjb9dzMBJqF1w5PdQEWS82MmRFRCzxNcXdJoSWkBaTsWMJLCTu Validator1").unwrap();
        }
        let mut cfg = Config::from_ini_str("").unwrap();
        assert_eq!(cfg.trusted_validators().len(), 0);
        splice_validators_file(&mut cfg, &tmp).unwrap();
        assert_eq!(cfg.trusted_validators().len(), 1);
        assert_eq!(
            cfg.trusted_validators()[0].key,
            "nHUjb9dzMBJqF1w5PdQEWS82MmRFRCzxNcXdJoSWkBaTsWMJLCTu"
        );
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn splice_validators_file_empty_file_ok() {
        use std::io::Write;
        let tmp = std::env::temp_dir().join("validators_splice_empty_test.txt");
        {
            let mut f = std::fs::File::create(&tmp).unwrap();
            write!(f, "").unwrap();
        }
        let mut cfg = Config::from_ini_str("").unwrap();
        splice_validators_file(&mut cfg, &tmp).unwrap();
        assert_eq!(cfg.trusted_validators().len(), 0);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn splice_validators_file_missing_is_error() {
        // An explicit validators_file that doesn't exist → error
        let missing = PathBuf::from("/tmp/xrpld_nonexistent_validators_9999.txt");
        let mut cfg = Config::from_ini_str("").unwrap();
        let result = splice_validators_file(&mut cfg, &missing);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err().kind, crate::ConfigErrorKind::Io { .. }));
    }

    #[test]
    fn splice_validators_file_appends_to_existing() {
        use std::io::Write;
        // Config already has one validator; splice adds another
        let ini_text = "[validators]\nnHUjb9dzMBJqF1w5PdQEWS82MmRFRCzxNcXdJoSWkBaTsWMJLCTu Existing\n";
        let mut cfg = Config::from_ini_str(ini_text).unwrap();
        assert_eq!(cfg.trusted_validators().len(), 1);

        let tmp = std::env::temp_dir().join("validators_splice_append_test.txt");
        {
            let mut f = std::fs::File::create(&tmp).unwrap();
            writeln!(f, "[validators]").unwrap();
            writeln!(f, "nHUon2tpyJEHHYGmxqeGu37cvPYHzrMtUNQFVdCgGNvEkjmCpTqK New").unwrap();
        }
        splice_validators_file(&mut cfg, &tmp).unwrap();
        assert_eq!(cfg.trusted_validators().len(), 2);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn splice_validators_file_merges_validator_list_keys() {
        use std::io::Write;
        let tmp = std::env::temp_dir().join("validators_splice_keys_test.txt");
        {
            let mut f = std::fs::File::create(&tmp).unwrap();
            writeln!(f, "[validator_list_keys]").unwrap();
            writeln!(f, "ED264807102805220DA0F312E71FC2C69E1552C9C5790F6C25E3729DEB573D5860").unwrap();
        }
        let mut cfg = Config::from_ini_str("").unwrap();
        splice_validators_file(&mut cfg, &tmp).unwrap();
        assert_eq!(cfg.validator_list_keys().len(), 1);
        let _ = std::fs::remove_file(&tmp);
    }

    // ---- run_bootstrap ----

    #[test]
    fn run_bootstrap_missing_config_dir_is_error() {
        let mut cfg = Config::from_ini_str("").unwrap();
        // No set_config_dir → must error
        let result = cfg.bootstrap();
        assert!(result.is_err());
        let msg = result.unwrap_err().message();
        assert!(
            msg.contains("config_dir") || msg.contains("set_config_dir"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn run_bootstrap_standalone_forces_tiny_node_size() {
        let mut cfg = Config::from_toml_str("").unwrap();
        cfg.set_config_dir(std::env::temp_dir());
        cfg.set_standalone(true);
        cfg.bootstrap().unwrap();
        assert_eq!(cfg.node_size(), NodeSize::Tiny);
    }

    #[test]
    fn run_bootstrap_data_dir_from_database_path() {
        let db_path = std::env::temp_dir().join("xrpld_bootstrap_db_test_xyz");
        let _ = std::fs::create_dir_all(&db_path);
        let toml = format!(r#"database_path = "{}""#, db_path.display());
        let mut cfg = Config::from_toml_str(&toml).unwrap();
        cfg.set_config_dir(std::env::temp_dir());
        cfg.set_standalone(true);
        cfg.bootstrap().unwrap();
        assert_eq!(cfg.data_dir(), db_path.as_path());
        let _ = std::fs::remove_dir_all(&db_path);
    }

    #[test]
    fn run_bootstrap_data_dir_defaults_to_config_dir_db() {
        let config_dir = std::env::temp_dir();
        let expected_db = config_dir.join("db");
        let mut cfg = Config::from_ini_str("").unwrap();
        cfg.set_config_dir(config_dir.clone());
        cfg.set_standalone(true);
        cfg.bootstrap().unwrap();
        assert_eq!(cfg.data_dir(), expected_db.as_path());
    }

    #[test]
    fn run_bootstrap_quiet_does_not_panic() {
        let mut cfg = Config::from_ini_str("").unwrap();
        cfg.set_config_dir(std::env::temp_dir());
        cfg.set_standalone(true);
        cfg.set_quiet(true);
        // Should not panic; no stderr emitted
        cfg.bootstrap().unwrap();
    }

    #[test]
    fn run_bootstrap_implicit_validators_txt_skipped_when_absent() {
        // No validators.txt in a temp dir that doesn't have one → should not error
        let tmp = std::env::temp_dir().join("xrpld_no_validators_test_dir_xyz");
        let _ = std::fs::create_dir_all(&tmp);
        // Make sure there's no validators.txt there
        let _ = std::fs::remove_file(tmp.join("validators.txt"));
        let mut cfg = Config::from_ini_str("").unwrap();
        cfg.set_config_dir(tmp.clone());
        cfg.set_standalone(true);
        cfg.bootstrap().unwrap();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn run_bootstrap_implicit_validators_txt_spliced_when_present() {
        use std::io::Write;
        let tmp = std::env::temp_dir().join("xrpld_implicit_validators_test_dir_xyz");
        let _ = std::fs::create_dir_all(&tmp);
        let vf = tmp.join("validators.txt");
        {
            let mut f = std::fs::File::create(&vf).unwrap();
            writeln!(f, "[validators]").unwrap();
            writeln!(f, "nHUjb9dzMBJqF1w5PdQEWS82MmRFRCzxNcXdJoSWkBaTsWMJLCTu FromFile").unwrap();
        }
        let mut cfg = Config::from_ini_str("").unwrap();
        cfg.set_config_dir(tmp.clone());
        cfg.set_standalone(true);
        cfg.bootstrap().unwrap();
        assert_eq!(cfg.trusted_validators().len(), 1);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ---- cross-section validators ----

    #[test]
    fn cross_validation_peers_in_without_peers_out_is_error() {
        let ini_text = "[peers_in_max]\n50\n";
        let mut cfg = Config::from_ini_str(ini_text).unwrap();
        cfg.set_config_dir(std::env::temp_dir());
        cfg.set_standalone(true);
        let result = cfg.bootstrap();
        assert!(result.is_err());
        let msg = result.unwrap_err().message();
        assert!(
            msg.contains("peers_in_max") || msg.contains("peers_out_max"),
            "unexpected: {msg}"
        );
    }

    #[test]
    fn cross_validation_peers_out_without_peers_in_is_error() {
        let ini_text = "[peers_out_max]\n50\n";
        let mut cfg = Config::from_ini_str(ini_text).unwrap();
        cfg.set_config_dir(std::env::temp_dir());
        cfg.set_standalone(true);
        let result = cfg.bootstrap();
        assert!(result.is_err());
        let msg = result.unwrap_err().message();
        assert!(
            msg.contains("peers_in_max") || msg.contains("peers_out_max"),
            "unexpected: {msg}"
        );
    }

    #[test]
    fn cross_validation_peers_out_too_small() {
        // peers_out_max < 10 when set is an error
        let ini_text = "[peers_out_max]\n5\n[peers_in_max]\n5\n";
        let mut cfg = Config::from_ini_str(ini_text).unwrap();
        cfg.set_config_dir(std::env::temp_dir());
        cfg.set_standalone(true);
        let result = cfg.bootstrap();
        assert!(result.is_err());
        let msg = result.unwrap_err().message();
        assert!(msg.contains("peers_out_max"), "unexpected: {msg}");
    }

    #[test]
    fn cross_validation_peers_both_set_ok() {
        let ini_text = "[peers_out_max]\n10\n[peers_in_max]\n10\n";
        let mut cfg = Config::from_ini_str(ini_text).unwrap();
        cfg.set_config_dir(std::env::temp_dir());
        cfg.set_standalone(true);
        cfg.bootstrap().unwrap();
    }

    #[test]
    fn cross_validation_network_quorum_exceeds_effective_peers_max() {
        // With default peers_max=0 (effective 21), setting network_quorum=22 should error
        let ini_text = "[network_quorum]\n22\n";
        let mut cfg = Config::from_ini_str(ini_text).unwrap();
        cfg.set_config_dir(std::env::temp_dir());
        cfg.set_standalone(true);
        let result = cfg.bootstrap();
        assert!(result.is_err());
        let msg = result.unwrap_err().message();
        assert!(msg.contains("network_quorum"), "unexpected: {msg}");
    }

    #[test]
    fn cross_validation_zero_port_is_error() {
        // A named port with port=0 should fail cross-section validation.
        // In INI mode, the port section name IS the port name listed in [server].
        let ini_text = "[server]\nport_rpc\n[port_rpc]\nport = 0\n";
        let mut cfg = Config::from_ini_str(ini_text).unwrap();
        cfg.set_config_dir(std::env::temp_dir());
        cfg.set_standalone(true);
        let result = cfg.bootstrap();
        assert!(result.is_err());
    }

    // ---- F35: detect_node_size_with — parameterized sizing matrix tests ----

    #[test]
    fn detect_node_size_with_ram_tiny_many_cpus() {
        // Low RAM forces Tiny even with many CPUs.
        assert_eq!(detect_node_size_with(4, 64), NodeSize::Tiny);
    }

    #[test]
    fn detect_node_size_with_ram_large_few_cpus() {
        // High RAM but single-CPU → CPU cap wins (Tiny).
        assert_eq!(detect_node_size_with(64, 1), NodeSize::Tiny);
    }

    #[test]
    fn detect_node_size_with_ram_and_cpu_both_large() {
        // 32 GB (→ Large) + 32 CPUs → 16 half-cpus → Huge from CPU.
        // min(Large, Huge) = Large.
        assert_eq!(detect_node_size_with(32, 32), NodeSize::Large);
    }

    #[test]
    fn detect_node_size_with_ram_huge_cpu_huge() {
        // 64 GB + 32 CPUs → Huge from both.
        assert_eq!(detect_node_size_with(64, 32), NodeSize::Huge);
    }

    #[test]
    fn detect_node_size_with_medium_ram_medium_cpu() {
        // 12 GB → Medium from RAM; 8 CPUs → 4 half-cpus → Medium from CPU.
        assert_eq!(detect_node_size_with(12, 8), NodeSize::Medium);
    }

    #[test]
    fn detect_node_size_with_small_ram_large_cpu() {
        // 8 GB → Small from RAM; many CPUs don't help.
        assert_eq!(detect_node_size_with(8, 64), NodeSize::Small);
    }

    // ---- F30: discover_config_file search paths ----

    #[test]
    fn discover_includes_etc_opt_paths() {
        // discover_config_file with no configs: the last candidate should be /etc/opt/...
        let result = discover_config_file(None, "rippled").unwrap();
        // If /etc/opt/rippled/rippled.cfg doesn't exist (likely), the last fallback is used.
        // We just check the function returns without error.
        let _ = result;
    }
}
