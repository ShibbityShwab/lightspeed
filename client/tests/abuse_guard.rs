//! Cross-crate guard: the client's tunneled-flow cap must stay strictly below
//! the relay's abuse threshold.
//!
//! The relay bans a source IP that reaches more than
//! `SecurityConfig::max_destinations_per_window` distinct destinations inside a
//! 10 s window, and holds that ban for 3600 s (`ban_duration_secs`). The blast
//! radius is every session behind the same NAT, so a client that relayed as many
//! distinct destinations as the threshold would get its own player banned.
//! `MAX_TUNNELED_FLOWS` therefore has to sit strictly below it.
//!
//! Neither number is restated here. The client crate cannot depend on the proxy
//! crate, so this guard reads the relay's default out of the relay's own source -
//! `default_max_destinations()` in `proxy/src/config.rs`, the function the
//! field's `#[serde(default = ...)]` names it by - and reads the client's cap out
//! of `client/src/interceptor/flows.rs`. Either side drifting into an unsafe
//! relation fails this file, with both values in the failure message.
//!
//! `include_str!` tracks the relay file, so editing the relay's default re-reads
//! (and re-judges) it here.

use lightspeed_client::interceptor::flows::{
    FlowMapConfig, MAX_TUNNELED_FLOWS, RELAY_MAX_DESTINATIONS_PER_WINDOW, WARN_TUNNELED_FLOWS,
};

/// The relay's config source, read at compile time so a change to its default is
/// tracked by this guard.
const RELAY_CONFIG_SRC: &str = include_str!("../../proxy/src/config.rs");

/// The relay function whose body carries the ban threshold.
const RELAY_DEFAULT_FN: &str = "fn default_max_destinations() -> usize {";

/// The attribute that binds the field to [`RELAY_DEFAULT_FN`].
const FIELD_SERDE_DEFAULT: &str = "#[serde(default = \"default_max_destinations\")]";

/// The field the threshold belongs to.
const FIELD_DECL: &str = "pub max_destinations_per_window: usize,";

/// The relay's declared default for `max_destinations_per_window`, read out of
/// `proxy/src/config.rs`.
///
/// A reshape that hides the number fails loudly rather than returning a stale
/// constant: a guard that stopped reading the relay would prove nothing.
fn relay_max_destinations_per_window() -> usize {
    let start = RELAY_CONFIG_SRC.find(RELAY_DEFAULT_FN).unwrap_or_else(|| {
        panic!(
            "proxy/src/config.rs no longer defines `{RELAY_DEFAULT_FN}`: the relay's \
                 `max_destinations_per_window` default moved, so this guard cannot read it"
        )
    });
    let body = &RELAY_CONFIG_SRC[start + RELAY_DEFAULT_FN.len()..];
    let end = body
        .find('}')
        .expect("the relay's default_max_destinations() has no closing brace");
    body[..end].trim().parse().unwrap_or_else(|e| {
        panic!(
            "the relay's default_max_destinations() body is not a usize literal: {:?} ({e})",
            &body[..end]
        )
    })
}

/// The guard must be reading the default the field actually binds, not a
/// same-named function the relay stopped using.
#[test]
fn the_guard_reads_the_default_the_field_binds() {
    let attr = RELAY_CONFIG_SRC
        .find(FIELD_SERDE_DEFAULT)
        .unwrap_or_else(|| {
            panic!(
                "proxy/src/config.rs no longer carries `{FIELD_SERDE_DEFAULT}`, so \
                 default_max_destinations() is not the default of that field"
            )
        });
    let decl = RELAY_CONFIG_SRC
        .find(FIELD_DECL)
        .unwrap_or_else(|| panic!("proxy/src/config.rs no longer declares `{FIELD_DECL}`"));
    assert!(
        attr < decl && decl - attr < 200,
        "the serde default attribute and the `max_destinations_per_window` field are no longer \
         adjacent in proxy/src/config.rs (attribute at byte {attr}, field at byte {decl}), so \
         this guard may be reading the wrong function"
    );
}

/// The cap, which never trips the relay's 3600 s source-IP ban.
#[test]
fn client_cap_stays_strictly_below_the_relay_ban_threshold() {
    let relay = relay_max_destinations_per_window();
    let cap = MAX_TUNNELED_FLOWS;

    assert!(
        cap < relay,
        "the client's tunneled-flow cap (MAX_TUNNELED_FLOWS = {cap} in \
         client/src/interceptor/flows.rs) must stay strictly below the relay's \
         max_destinations_per_window default ({relay} in proxy/src/config.rs): at {cap} >= {relay} \
         one client can relay as many distinct destinations as trip the relay's \
         3600 s source-IP ban"
    );
}

/// The mirror constant in `flows.rs` names the relay rule, so it must equal the
/// relay's declared default - otherwise its doc comment lies about what bounds.
#[test]
fn client_mirror_equals_the_relay_default() {
    let relay = relay_max_destinations_per_window();

    assert_eq!(
        RELAY_MAX_DESTINATIONS_PER_WINDOW, relay,
        "client/src/interceptor/flows.rs mirrors the relay default as \
         RELAY_MAX_DESTINATIONS_PER_WINDOW = {RELAY_MAX_DESTINATIONS_PER_WINDOW}, but \
         proxy/src/config.rs declares {relay}"
    );
}

/// The warning has to arrive before the cap binds, or the user hears about the
/// ban risk only after the client already stopped tunneling. Read through the
/// map's config - the runtime numbers the map actually uses, as `flows.rs` does
/// for its own arithmetic - rather than comparing two constants clippy folds.
#[test]
fn warning_fires_before_the_cap_binds() {
    let cfg = FlowMapConfig::default();

    assert!(
        cfg.warn_tunneled < cfg.max_tunneled,
        "WARN_TUNNELED_FLOWS ({}) must fire before MAX_TUNNELED_FLOWS ({}), both in \
         client/src/interceptor/flows.rs",
        cfg.warn_tunneled,
        cfg.max_tunneled
    );
    assert_eq!(
        cfg.max_tunneled, MAX_TUNNELED_FLOWS,
        "the map's runtime cap must be the constant this guard binds, not a copy of it"
    );
    assert_eq!(
        cfg.warn_tunneled, WARN_TUNNELED_FLOWS,
        "the map's runtime warning threshold must be the constant this guard binds"
    );
}
