//! CLI flag mirror type.
//!
//! `CliFlags` is a plain, logic-free 1:1 mirror of the command-line options
//! that affect node behaviour.  All fields are `Option`/`bool` with sensible
//! defaults; no invariants are enforced here.  [`Config::apply_cli_flags`]
//! translates these into the appropriate `Config` fields and validates
//! combinations.

/// Mirror of the command-line flags parsed by `Main.cpp`.
///
/// Every field corresponds to one CLI option.  The struct is deliberately
/// logic-free: it merely carries what the command line said.
#[derive(Debug, Clone, Default)]
pub struct CliFlags {
    /// `--standalone` / `-a`: run without connecting to the network.
    pub standalone: bool,
    /// `--quiet` / `-q`: suppress informational output.
    pub quiet: bool,
    /// `--silent`: suppress all output (implies `--quiet`).
    pub silent: bool,
    /// `--quorum <n>`: override the validation quorum.  Must be non-zero.
    pub quorum: Option<u32>,
    /// `--start`: start with a fresh ledger (`StartUpType::Fresh`).
    pub start: bool,
    /// `--ledger <hash-or-seq>`: load or replay starting from this ledger.
    pub ledger: Option<String>,
    /// `--ledgerfile <path>`: load ledger from a file (`StartUpType::LoadFile`).
    pub ledger_file: Option<String>,
    /// `--load`: force `StartUpType::Load`.
    pub load: bool,
    /// `--net`: start in network mode.
    pub net: bool,
    /// `--replay`: used together with `--ledger` to replay the ledger.
    pub replay: bool,
    /// `--trap_tx_hash <hash>`: trap a specific transaction during replay.
    pub trap_tx_hash: Option<String>,
    /// `--valid`: start with `START_VALID = true`.
    pub valid: bool,
    /// `--import`: set `doImport = true`.
    pub import: bool,
    /// `--force_ledger_present_range <min,max>`: raw comma-separated string;
    /// parsed into `(u32, u32)` by `Config::apply_cli_flags`.
    pub force_ledger_present_range: Option<String>,
    /// `--rpc_ip <endpoint>`: override the RPC destination IP address.
    pub rpc_ip: Option<String>,
    /// `--rpc_port <port>`: deprecated; used only when `rpc_ip` has no port.
    pub rpc_port: Option<u16>,
    /// `--nodeid <id>`: specify the node identity.
    pub nodeid: Option<String>,
    /// `--newnodeid`: generate a new node identity.
    pub newnodeid: bool,
}

impl CliFlags {
    /// Set the `standalone` flag.
    pub fn set_standalone(&mut self, value: bool) {
        self.standalone = value;
    }

    /// Set the `quiet` flag.
    pub fn set_quiet(&mut self, value: bool) {
        self.quiet = value;
    }

    /// Set the `silent` flag.
    pub fn set_silent(&mut self, value: bool) {
        self.silent = value;
    }

    /// Set the `quorum` override.  Stores `Some(value)`; a value of `0` will
    /// be rejected by `apply_cli_flags`.
    pub fn set_quorum(&mut self, value: u32) {
        self.quorum = Some(value);
    }

    /// Set the `start` flag.
    pub fn set_start(&mut self, value: bool) {
        self.start = value;
    }

    /// Set the `ledger` option.
    pub fn set_ledger(&mut self, value: &str) {
        self.ledger = Some(value.to_owned());
    }

    /// Set the `ledgerfile` option.
    pub fn set_ledgerfile(&mut self, value: &str) {
        self.ledger_file = Some(value.to_owned());
    }

    /// Set the `load` flag.
    pub fn set_load(&mut self, value: bool) {
        self.load = value;
    }

    /// Set the `net` flag.
    pub fn set_net(&mut self, value: bool) {
        self.net = value;
    }

    /// Set the `replay` flag.
    pub fn set_replay(&mut self, value: bool) {
        self.replay = value;
    }

    /// Set the `trap_tx_hash` option.
    pub fn set_trap_tx_hash(&mut self, value: &str) {
        self.trap_tx_hash = Some(value.to_owned());
    }

    /// Set the `valid` flag.
    pub fn set_valid(&mut self, value: bool) {
        self.valid = value;
    }

    /// Set the `import` flag.
    pub fn set_import(&mut self, value: bool) {
        self.import = value;
    }

    /// Set the `force_ledger_present_range` raw string.
    pub fn set_force_ledger_present_range(&mut self, value: &str) {
        self.force_ledger_present_range = Some(value.to_owned());
    }

    /// Set the `rpc_ip` option.
    pub fn set_rpc_ip(&mut self, value: &str) {
        self.rpc_ip = Some(value.to_owned());
    }

    /// Set the `rpc_port` option.
    pub fn set_rpc_port(&mut self, value: u16) {
        self.rpc_port = Some(value);
    }

    /// Set the `nodeid` option.
    pub fn set_nodeid(&mut self, value: &str) {
        self.nodeid = Some(value.to_owned());
    }

    /// Set the `newnodeid` flag.
    pub fn set_newnodeid(&mut self, value: bool) {
        self.newnodeid = value;
    }
}
