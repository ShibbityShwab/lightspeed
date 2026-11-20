//! GUI config adapter — the model and its I/O live in the shared crate.
//!
//! MOVED at WF-081 W2-1 stage S1 (`docs/design/gui-core-boundary.md` §5.1/§6):
//! `AppConfig`, its TOML parser/serializer, the tolerant loader (now carrying
//! the [`ConfigRecovery`] diagnostic), the reset helper and the discovery merge
//! all live in `lightspeed_client::app::config`, so the CLI can adopt the same
//! schema at S6. The GUI keeps only its file paths
//! ([`crate::paths::config_paths`]) and this re-export, so a config fix lands
//! once instead of twice.
//!
//! WF-081 W2-3b S6c (owner Q10) adds the canonical/legacy pair: the GUI reads
//! `lightspeed.toml` and imports the pre-unification `config.toml` read-only
//! through [`load_unified`], writes only [`save_canonical`], and never touches
//! the legacy file.
//!
//! The retired `retry_after_fallback` key is part of the shared parser's
//! contract: tolerated on read (unknown keys are ignored), never written. The
//! shared crate's tests pin both directions; this shell deliberately adds no
//! second copy of the schema.

pub use lightspeed_client::app::config::{
    load_unified, merge_discovered, reset_to_defaults, save_canonical, AppConfig, ConfigRecovery,
    ProxyEntry,
};
