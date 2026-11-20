//! The pure route-surface adapter: `AppStatus` → the §2 word/rows model.
//!
//! WF-081 W2-1 S3a (`docs/design/gui-refresh-spec.md` §2 is the contract of
//! record, amendments A1–A13). This module is the **presentation seam**: it maps
//! the shared status snapshot to exactly the words, metric, sub-line, notice,
//! rows and disclosure rows the shell renders. It decides nothing about the
//! session: no thresholds, verdicts, joins, counters or route changes — every
//! fact, including the warning, comes from `AppStatus`.
//!
//! Contract rules encoded here:
//! - one word, one driving fact (§2.1 precedence): Error → NO RETURN PATH →
//!   HOLDING → OPTIMIZING → CONNECTING → CONNECTED / NOT CONNECTED;
//! - Choosing is a sub-state of HOLDING, not a word (A4); Needs elevation is an
//!   overlay, never a word (A6);
//! - warn-only: a starvation warning never changes the route, and there is no
//!   "recovered" state, banner or word — the warning clearing and the word
//!   returning to OPTIMIZING *is* the observable effect (A7);
//! - `Relayed` is one source pair ([`RelayedPair`]); its starving flag never
//!   flips the word (the watchdog owns the warning; A5/C3);
//! - relay-measured rows lead, the four estimator rows are the fallback, and
//!   the location ladder is display-only (A8/A9).
//!
//! The adapter returns catalogue keys plus their placeholder values, not
//! rendered copy: `i18n` stays a shell concern, so the same fixture asserts the
//! same contract in all nine locales.
//!
//! Every item here is consumed by the W2-2 shell in `app.rs` (or by the
//! contract fixtures in this module); there is no module-wide `dead_code`
//! allowance. W2-4b retired the last duplicate copy builders and removed the
//! blanket, annotating only the genuine platform exemptions elsewhere.

use lightspeed_client::app::dto::{
    AppErrorKind, AppStatus, ClientRoute, LocationRung, RelayedPair, ServerLegState, TrayState,
};
use lightspeed_client::app::status::{action_slot, ActionSlot};

// ── The model ──────────────────────────────────────────────────────────────

/// The nine route rows of the contract, as seven words (Idle contributes two and
/// `NO RETURN PATH` is the warning row). Trace-only or overlay facts get no
/// variant (A6/A7); there is deliberately no `Direct`, `Retesting`,
/// `ServerRejectsRelay` or `Recovered` variant — A7 retired them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteWord {
    /// Idle, no relay link: `state.disconnected`.
    NotConnected,
    /// Idle, relay link up: `state.connected`.
    Connected,
    /// A connect attempt is in flight: `state.connecting` (DTO gap closed here).
    Connecting,
    /// The engine withholds tunnelling while it finds/chooses: `state.holding`.
    Holding,
    /// A tunnel carries traffic and no warning is active: `state.optimizing`.
    Optimizing,
    /// The return path is judged faulty while relaying: `state.not_returning`.
    NoReturnPath,
    /// A live failure: `state.error`.
    Error,
}

impl RouteWord {
    /// The catalogue key for the word (§2.2 rows 1–8).
    pub fn key(self) -> &'static str {
        match self {
            Self::NotConnected => "state.disconnected",
            Self::Connected => "state.connected",
            Self::Connecting => "state.connecting",
            Self::Holding => "state.holding",
            Self::Optimizing => "state.optimizing",
            Self::NoReturnPath => "state.not_returning",
            Self::Error => "state.error",
        }
    }

    /// The one tone the word paints (colour is redundant with the word, never
    /// the carrier of the state).
    pub fn tone(self) -> Tone {
        match self {
            Self::NoReturnPath | Self::Error => Tone::Danger,
            Self::Holding => Tone::Warn,
            Self::Optimizing => Tone::Signal,
            Self::Connected | Self::Connecting | Self::NotConnected => Tone::Muted,
        }
    }
}

/// The word/notice tones the shell paints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Neutral ink.
    Muted,
    /// The one bright state (traffic is being relayed).
    Signal,
    /// Degraded but working.
    Warn,
    /// Danger: the warning row or an error.
    Danger,
}

/// A catalogued line: a key plus its placeholder values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// The catalogue key.
    pub key: &'static str,
    /// `(placeholder, value)` pairs, in catalogue order.
    pub args: Vec<(&'static str, String)>,
}

impl Line {
    /// A line with no placeholders.
    pub fn new(key: &'static str) -> Self {
        Self {
            key,
            args: Vec::new(),
        }
    }

    /// A line with placeholder values.
    pub fn with(key: &'static str, args: &[(&'static str, String)]) -> Self {
        Self {
            key,
            args: args.to_vec(),
        }
    }
}

/// The rail's live metric: the relay leg, visibly labelled (§1.2/A13).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveMetric {
    /// The relay leg in whole milliseconds; `None` renders `-- ms`.
    pub value_ms: Option<u32>,
    /// The leg caption (always names the leg; A13).
    pub caption_key: &'static str,
}

/// One ledger row: a label key plus its value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The row label key.
    pub label_key: &'static str,
    /// The row value.
    pub value: RowValue,
}

/// A row value: data, copy with placeholders, a hint, or a warning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowValue {
    /// Genuine data (addresses, labels, counters): mono in the shell.
    Data(String),
    /// Catalogued copy with values.
    Copy(Line),
    /// A dim hint where there is no fact to show.
    Hint(&'static str),
    /// A value that is working but degraded (warn tone).
    Warn(Line),
}

/// The action slot: one label, enabled state and the in-flight state (§2.2/§3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionSurface {
    /// The label key (STOP OPTIMIZING / restart as administrator / OPTIMIZE MY
    /// ROUTE / STARTING…).
    pub label_key: &'static str,
    /// Whether the slot can act right now.
    pub enabled: bool,
    /// The destructive treatment (stop) vs the one bright fill.
    pub danger: bool,
    /// A session command is in flight: the loading state, not re-triggerable.
    pub in_flight: bool,
    /// Why the slot is disabled: the first unmet precondition in gate order
    /// (relay → connecting → registration). `None` while enabled.
    pub disabled_reason: Option<Line>,
}

/// The overlay facts: Needs elevation is an overlay, never a word (A6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlay {
    /// No overlay.
    None,
    /// The process cannot relay; the action invites elevation and a notice says
    /// why. The route word is unchanged.
    NeedsElevation,
}

/// The location ladder block (§2.4, A8): display-only except the cloud point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocationBlock {
    /// The value label key (`Location`; new in W2-2).
    pub label_key: &'static str,
    /// The rung's caption: a catalogued line, so a template with a placeholder
    /// (`route.location_geofeed` carries `{place}`) is never emitted bare.
    pub rung: Line,
    /// The provider region / place / network detail, when the rung has one.
    pub detail: Option<RowValue>,
    /// The attribution/as-of line (geofeed).
    pub note: Option<RowValue>,
    /// The "shown as-is" note (geofeed).
    pub as_is: Option<&'static str>,
    /// The "free databases are unreliable" hint (hosting/unknown).
    pub hint: Option<&'static str>,
    /// The geofeed source URL, for the hover.
    pub source: Option<Line>,
}

/// The client route trace (A10): a placement anchor, not a ping, not a city.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteTraceSurface {
    /// The caption: the shipped `route.trace_route` key with its hop count.
    pub caption: Line,
    /// The shipped `route.trace_nearest` suffix, when a router answered.
    pub nearest: Option<Line>,
    /// The last responding hop's network name, when a shipped location index
    /// knows it: a cloud provider, an operator geofeed or a hosting network —
    /// never a city and never a country estimate (A10 display-only provenance).
    pub provider: Option<String>,
}

/// The whole route surface for one frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteSurface {
    /// The one state word and its key/tone.
    pub word: RouteWord,
    /// The live metric row (relay leg).
    pub metric: LiveMetric,
    /// The one-line description under the word.
    pub subline: Option<Line>,
    /// The warning line inside the route card (danger) — one at a time (§2.9).
    pub warning: Option<Line>,
    /// The notice line in the scrolling region (warn), e.g. elevation.
    pub notice: Option<Line>,
    /// The elevation overlay (A6).
    pub overlay: Overlay,
    /// The one action slot.
    pub action: ActionSurface,
    /// The default ledger rows (§3: ≤6 on the default surface).
    pub ledger: Vec<Row>,
    /// The `Details` disclosure rows (server leg variants, ping, trace).
    pub details: Vec<Row>,
    /// The location ladder block, when a server is known.
    pub location: Option<LocationBlock>,
    /// The client route-trace caption, when a trace exists.
    pub trace: Option<RouteTraceSurface>,
    /// The tray projection of this same snapshot (never `Optimizing` while a
    /// warning is active).
    pub tray: TrayState,
    /// The relay-recovered banner is **never** rendered (A7: the recovery is the
    /// warning clearing, trace-only). Always `false`; a test pins the absence.
    pub relay_recovered: bool,
}

/// Map one [`AppStatus`] snapshot to the surface model.
pub fn route_surface(status: &AppStatus) -> RouteSurface {
    let boosting = status.boosting;
    let warning = status.watchdog.warning.filter(|_| boosting);
    let word = word_for(status, boosting, warning.is_some());
    // The Error row's sub-line is the taxonomy sentence (spec §2.2 row 8); the
    // same sentence is repeated behind Details with its verbatim detail.
    let subline = match status.error.as_ref() {
        Some(error) => Some(error_line(error)),
        None => subline_for(status, boosting, warning.is_some()),
    };
    RouteSurface {
        word,
        metric: metric_for(status),
        subline,
        warning: warning.map(|warning| {
            Line::new(match warning.fault {
                lightspeed_client::app::dto::ReturnPathFault::NoReplyFromRelay
                | lightspeed_client::app::dto::ReturnPathFault::TooFewReplies => {
                    "route.relay_not_returning"
                }
                lightspeed_client::app::dto::ReturnPathFault::InjectionFailed => {
                    "route.injection_failed_warning"
                }
            })
        }),
        notice: notice_for(status),
        overlay: overlay_for(status),
        action: action_for(status, boosting),
        ledger: ledger_rows(status),
        details: detail_rows(status),
        location: location_block(status),
        trace: trace_surface(status),
        tray: status.tray,
        relay_recovered: false,
    }
}

/// §2.1 precedence: Error → NO RETURN PATH → HOLDING → OPTIMIZING → CONNECTING
/// → CONNECTED / NOT CONNECTED.
fn word_for(status: &AppStatus, boosting: bool, warning_active: bool) -> RouteWord {
    if status.error.is_some() {
        RouteWord::Error
    } else if warning_active {
        RouteWord::NoReturnPath
    } else if boosting && status.engine.tunnelling_enabled == Some(false) {
        RouteWord::Holding
    } else if boosting {
        RouteWord::Optimizing
    } else if status.relay.connecting {
        RouteWord::Connecting
    } else if status.engine.connected {
        RouteWord::Connected
    } else {
        RouteWord::NotConnected
    }
}

/// The rail metric is the relay leg, always labelled (A13) — never the game's
/// ping and never the direct ping when fresh.
fn metric_for(status: &AppStatus) -> LiveMetric {
    let value_ms = if status.engine.connected && status.engine.latest_rtt_ms > 0.0 {
        Some(status.engine.latest_rtt_ms.round() as u32)
    } else {
        None
    };
    LiveMetric {
        value_ms,
        caption_key: "route.leg_relay",
    }
}

fn subline_for(status: &AppStatus, boosting: bool, warning_active: bool) -> Option<Line> {
    if warning_active {
        // The route keeps being named while the warning is up: the player is
        // still relayed through this relay (§2.2 row 6).
        let relay = relay_label(status);
        let game = status.engine.redirect_game.clone();
        return Some(Line::with(
            "route.relaying_route",
            &[("game", game), ("relay", relay)],
        ));
    }
    if boosting {
        if status.engine.tunnelling_enabled == Some(false) {
            // Spec §2.2 row 3 order: the scan first (no server locked yet),
            // then the destination-aware choice, then the plain hold.
            if status.relay.server.is_none() {
                return Some(Line::new("route.holding_scan"));
            }
            if status.relay.choosing {
                return Some(Line::new("route.choosing"));
            }
            return Some(Line::new("route.holding"));
        }
        return Some(Line::with(
            "route.relaying_route",
            &[
                ("game", status.engine.redirect_game.clone()),
                ("relay", relay_label(status)),
            ],
        ));
    }
    if status.relay.connecting {
        return Some(Line::with(
            "route.connecting",
            &[("relay", relay_label(status))],
        ));
    }
    if status.engine.connected {
        return Some(match status.engine.redirect_game.as_str() {
            "" => Line::new("route.relay_ready_pick"),
            name => Line::with("route.relay_ready_named", &[("name", name.to_string())]),
        });
    }
    Some(Line::new("route.no_relay"))
}

fn relay_label(status: &AppStatus) -> String {
    match (status.relay.relay, status.engine.proxy_addr.as_str()) {
        (Some(relay), _) => relay.to_string(),
        (None, "") => "--".to_string(),
        (None, proxy) => proxy.to_string(),
    }
}

/// The elevation notice is the warn line above the config region (§2.2 row 7).
fn notice_for(status: &AppStatus) -> Option<Line> {
    match overlay_for(status) {
        Overlay::NeedsElevation => Some(Line::new("notice.needs_admin")),
        Overlay::None => None,
    }
}

/// Needs elevation is an overlay driven by `Capabilities.elevated` (A6); the
/// invite appears only when a relay is selected **and the boost can otherwise
/// start** (spec M1, §2.2 row 7): unelevated plus disconnected, or unelevated
/// with no relay, offers no elevation invitation because there is nothing to
/// elevate for.
fn overlay_for(status: &AppStatus) -> Overlay {
    if !status.capabilities.elevated
        && status.relay.relay.is_some()
        && status.engine.connected
        && !status.boosting
        && status.error.is_none()
        && !status.relay.connecting
    {
        Overlay::NeedsElevation
    } else {
        Overlay::None
    }
}

fn action_for(status: &AppStatus, boosting: bool) -> ActionSurface {
    let has_relay = status.relay.relay.is_some();
    let slot = action_slot(
        status.capabilities.elevated,
        boosting,
        status.engine.connected,
        has_relay,
    );
    // The loading state is a real fact: a command in flight (§2.1).
    let start_in_flight = matches!(
        status.session.in_flight,
        Some(lightspeed_client::app::dto::SessionCommand::StartBoost)
    );
    let (label_key, enabled, danger) = if start_in_flight {
        ("action.starting", false, false)
    } else if boosting {
        ("action.stop_optimizing", true, true)
    } else {
        match slot {
            ActionSlot::RestartElevated => ("action.restart_admin_optimize", true, false),
            // The elevation overlay owns the label when it is showing.
            ActionSlot::StartReady => ("action.optimize_route", true, false),
            ActionSlot::StartBlocked => ("action.optimize_route", false, false),
            ActionSlot::StopOptimizing => ("action.stop_optimizing", true, true),
        }
    };
    let disabled_reason = if enabled || start_in_flight {
        None
    } else if !has_relay {
        Some(Line::new("action_reason.missing_relay"))
    } else if status.relay.connecting {
        Some(Line::new("action_reason.waiting_relay"))
    } else if status.engine.registration_error.is_some() {
        Some(Line::new("action_reason.registration"))
    } else {
        // The action is gated on the engine's link; the reason is the link.
        Some(Line::new("action_reason.not_connected"))
    };
    ActionSurface {
        label_key,
        enabled,
        danger,
        in_flight: start_in_flight,
        disabled_reason,
    }
}

/// The default ledger: what the player can act on and the session's own count.
fn ledger_rows(status: &AppStatus) -> Vec<Row> {
    let boosting = status.boosting;
    let mut rows = Vec::new();
    rows.push(Row {
        label_key: "route.relay",
        value: relay_row_value(status),
    });
    // Spec §2.2 row 1: `Relay RTT` when measured. The rail metric shows the
    // same relay leg, but the §3 ledger matrix keeps this row, so the seam
    // emits it rather than leaving W2-2 to invent it.
    if status.engine.connected && status.engine.latest_rtt_ms > 0.0 {
        rows.push(Row {
            label_key: "route.relay_rtt",
            value: RowValue::Data(format!("{:.0} ms", status.engine.latest_rtt_ms)),
        });
    }
    if boosting && status.engine.tunnelling_enabled == Some(false) {
        rows.push(Row {
            label_key: "route.path",
            value: RowValue::Warn(Line::new("state.holding")),
        });
    }
    // The single best Server leg row lives on the default surface; every other
    // leg/trace row is behind Details (§2.3/§3).
    if let Some(leg) = best_server_leg_row(status) {
        rows.push(leg);
    }
    rows.push(Row {
        label_key: "route.game_server",
        value: game_server_value(status),
    });
    rows.push(Row {
        label_key: "route.relayed",
        value: relayed_value(status),
    });
    rows
}

fn relay_row_value(status: &AppStatus) -> RowValue {
    if !status.engine.connected {
        return RowValue::Hint("--");
    }
    match status.relay.relay {
        Some(relay) => RowValue::Data(relay.to_string()),
        None => RowValue::Data(status.engine.proxy_addr.clone()),
    }
}

fn game_server_value(status: &AppStatus) -> RowValue {
    match status.relay.server {
        Some(server) => RowValue::Data(server.to_string()),
        None => RowValue::Hint("route.place_hint"),
    }
}

/// The `Relayed` row from the DTO's one source pair (§2.2 row 5, A5).
fn relayed_value(status: &AppStatus) -> RowValue {
    // Spec §2.2 row 3 / §3: while the session is held before a relay is chosen
    // the row says `No traffic relayed yet` instead of a 0 up · 0 down pair.
    let holding = status.boosting && status.engine.tunnelling_enabled == Some(false);
    if holding {
        let nothing_yet = status
            .relayed
            .map(|pair| pair.sent == 0 && pair.returned == 0)
            .unwrap_or(true);
        if nothing_yet {
            return RowValue::Hint("route.relayed_hold");
        }
    }
    let Some(RelayedPair { sent, returned }) = status.relayed else {
        return RowValue::Hint("route.count_hint");
    };
    // One source pair: the row and the starving flag share the same counters.
    let pair = RelayedPair { sent, returned };
    let line = Line::with(
        "route.sent_returned",
        &[
            ("sent", sent.to_string()),
            ("returned", returned.to_string()),
        ],
    );
    if pair.starving() {
        RowValue::Warn(line)
    } else {
        RowValue::Copy(line)
    }
}

/// The relay-measured `Server leg` row when the relay has a report for this
/// pair (A9, leads over the estimator).
fn best_server_leg_row(status: &AppStatus) -> Option<Row> {
    let leg = status.server_leg.icmp.as_ref()?;
    let key = if leg.validated {
        "route.server_leg_relay_measured"
    } else {
        "route.server_leg_relay_measured_unvalidated"
    };
    Some(Row {
        label_key: "route.server_leg",
        value: RowValue::Copy(Line::with(
            key,
            &[
                ("ms", format!("{:.0}", f64::from(leg.median_us) / 1000.0)),
                ("n", leg.samples.to_string()),
                ("secs", leg.age.as_secs().to_string()),
            ],
        )),
    })
}

/// The disclosure rows: direct ping, expected path, relay app-RTT, relay trace,
/// estimator fallback, triangle verdict, client route trace.
fn detail_rows(status: &AppStatus) -> Vec<Row> {
    // §2.3: the server-leg and trace rows exist only while a tunnel carries
    // traffic; with the tunnel down there is nothing to state and nothing to
    // invent.
    let mut rows = Vec::new();
    // The direct ping is a client measurement of the locked server, not a
    // tunnel fact: the Details contents list it without a boosting condition,
    // so a connected-but-idle window that has one still shows its age. (QA
    // R15: the rest of the disclosure is tunnel-scoped; this row is not.)
    if let Some(ping) = status.relay.server_ping {
        rows.push(Row {
            label_key: "route.server_ping",
            value: RowValue::Data(format!("{:.0} ms \u{00b7} {}s", ping.ms, ping.age_secs)),
        });
    }
    if !status.boosting {
        return rows;
    }
    // Relay-measured rows lead; the estimator rows are the fallback (§2.3/A9).
    if let Some(expected_ms) = status.server_leg.expected_path_ms {
        rows.push(Row {
            label_key: "route.expected_path_label",
            value: RowValue::Copy(Line::with(
                "route.expected_path",
                &[("ms", format!("{expected_ms:.0}"))],
            )),
        });
    }
    if let Some(app) = status.server_leg.app.as_ref() {
        rows.push(Row {
            label_key: "route.server_leg_app",
            value: RowValue::Copy(Line::with(
                "route.server_leg_app_measured",
                &[
                    (
                        "ms",
                        format!("{:.0}", f64::from(app.passive_median_us) / 1000.0),
                    ),
                    ("n", app.samples.to_string()),
                ],
            )),
        });
    }
    if status.server_leg.icmp.is_none() {
        // The four v1 estimator rows are the fallback when the relay has **no
        // report** for this pair (A9). Keyed on the report, not on the trace: a
        // relay can publish a trace while no ICMP leg report exists, and that
        // reachable state must still show the fallback row.
        rows.push(estimator_row(status.relay.server_leg));
    }
    if let Some(trace) = status.server_leg.trace.as_ref() {
        rows.push(Row {
            label_key: "route.route_trace",
            value: relay_trace_value(trace),
        });
    }
    if let Some((client_us, direct_us)) = status.relay.cannot_beat_direct {
        rows.push(Row {
            label_key: "route.cannot_beat_direct",
            // Caption as the label; the provable arithmetic as the value. The
            // shipped caption has no placeholders, so the two measurements are
            // emitted as data rather than silently dropped into it.
            value: RowValue::Data(format!(
                "{} ms \u{2265} {} ms",
                client_us / 1000,
                direct_us / 1000
            )),
        });
    }
    rows
}

/// The relay's TTL-trace row: hops plus the last router's RTT when one answered.
fn relay_trace_value(trace: &lightspeed_client::ServerTraceReport) -> RowValue {
    match trace.last_hop_rtt_us {
        Some(rtt_us) => RowValue::Copy(Line::with(
            "route.route_trace_hops",
            &[
                ("hops", trace.hops.to_string()),
                ("ms", (f64::from(rtt_us) / 1000.0).round().to_string()),
            ],
        )),
        None => RowValue::Copy(Line::with(
            "route.route_trace_hops_only",
            &[("hops", trace.hops.to_string())],
        )),
    }
}

/// One of the four v1 estimator `Server leg` rows (§2.3, A9 fallback).
fn estimator_row(state: ServerLegState) -> Row {
    let value = match state {
        ServerLegState::Measured(measured) => {
            let mut args = vec![
                ("ms", format!("{:.0}", measured.dest_leg_us as f64 / 1000.0)),
                ("n", measured.samples.to_string()),
            ];
            match measured.age {
                Some(age) => {
                    args.push(("secs", age.as_secs().to_string()));
                    RowValue::Copy(Line::with("route.server_leg_measured_age", &args))
                }
                None => RowValue::Copy(Line::with("route.server_leg_measured", &args)),
            }
        }
        ServerLegState::Bounded { floor_us } => RowValue::Copy(Line::with(
            "route.server_leg_bounded",
            &[("ms", format!("{:.0}", floor_us as f64 / 1000.0))],
        )),
        ServerLegState::Prior { prior_us } => RowValue::Copy(Line::with(
            "route.server_leg_prior",
            &[
                ("prior", format!("{:.0}", prior_us as f64 / 1000.0)),
                (
                    "unc",
                    format!(
                        "{:.0}",
                        lightspeed_client::PRIOR_UNCERTAINTY_US as f64 / 1000.0
                    ),
                ),
            ],
        )),
        ServerLegState::Unknown => RowValue::Hint("route.server_leg_unknown"),
    };
    Row {
        label_key: "route.server_leg",
        value,
    }
}

/// The location ladder block: five mutually exclusive rungs, display-only
/// except the cloud point (A8). The rung is [`LocationStatus::rung`] — one
/// source; the surface never changes the rung it was handed.
fn location_block(status: &AppStatus) -> Option<LocationBlock> {
    // No game server on screen: no location block at all (never a blank row).
    status.relay.server?;
    let location = &status.location;
    let mut block = LocationBlock {
        label_key: "route.location_label",
        rung: Line::new("route.location_unknown"),
        detail: None,
        note: None,
        as_is: None,
        hint: None,
        source: None,
    };
    match location.rung {
        LocationRung::Cloud => {
            block.rung = Line::new("route.location_provider");
            // Only what the source has: a missing region omits the separator
            // and the empty field rather than printing a dangling dot.
            block.detail = match (location.provider, location.region) {
                (Some(provider), Some(region)) if !provider.is_empty() && !region.is_empty() => {
                    Some(RowValue::Data(format!(
                        "{} \u{00b7} {region}",
                        provider.to_uppercase()
                    )))
                }
                (Some(provider), _) if !provider.is_empty() => {
                    Some(RowValue::Data(provider.to_uppercase()))
                }
                _ => None,
            };
        }
        LocationRung::Geofeed => {
            let geofeed = location.geofeed?;
            let place = crate::server_place::geofeed_place_text(
                geofeed.country,
                geofeed.region,
                geofeed.city,
            );
            // The `{place}` template belongs to the rung caption, so the caption
            // is a `Line` with its placeholder supplied instead of a bare key
            // that would leak `{place}` into the shell.
            block.rung = Line::with("route.location_geofeed", &[("place", place)]);
            block.detail = Some(RowValue::Copy(Line::with(
                "route.location_geofeed_detail",
                &[
                    ("provider", geofeed.provider.to_string()),
                    ("date", geofeed.as_of.to_string()),
                ],
            )));
            block.as_is = Some("route.location_geofeed_note");
            block.source = Some(Line::with(
                "route.location_geofeed_source",
                &[("source", geofeed.source.to_string())],
            ));
        }
        LocationRung::Hosting => {
            // A hosting rung without both facts cannot state its network: show
            // the honest unknown instead of `AS0` / a blank name.
            match (location.asn, location.hosting) {
                (Some(asn), Some(name)) if asn > 0 && !name.is_empty() => {
                    block.rung = Line::new("route.location_hosting");
                    block.detail = Some(RowValue::Data(format!("AS{asn} \u{00b7} {name}")));
                    block.hint = Some("route.location_hosting_hint");
                }
                _ => {
                    block.rung = Line::new("route.location_unknown");
                    block.hint = Some("route.location_hosting_hint");
                }
            }
        }
        LocationRung::Country => {
            block.rung = Line::new("route.location_estimate");
        }
        LocationRung::Unknown => {
            block.rung = Line::new("route.location_unknown");
            block.hint = Some("route.location_hosting_hint");
        }
    }
    Some(block)
}

/// The client route-trace caption (A10): the shipped `Route to server` key with
/// hops, the shipped `nearest responding hop` suffix when one answered, and the
/// last responding hop's provider/hosting name when a shipped location index
/// knows it.
///
/// A trace with no responding hop (timeout / 0 hops) is **no row at all**: the
/// shipped caption helper returned nothing for it, and rendering
/// `Route to server: 0 hops` would claim a measurement the probe never made.
fn trace_surface(status: &AppStatus) -> Option<RouteTraceSurface> {
    let route = status.route_trace?;
    if route.hops == 0 {
        return None;
    }
    let caption = Line::with("route.trace_route", &[("hops", route.hops.to_string())]);
    let nearest = route
        .last_hop_rtt_ms
        .map(|ms| Line::with("route.trace_nearest", &[("ms", ms.to_string())]));
    Some(RouteTraceSurface {
        caption,
        nearest,
        provider: last_hop_provider(&route),
    })
}

/// The provider/hosting network name for the trace's last responding hop, from
/// the shipped location indexes ([`lightspeed_client::app::location::place`]).
///
/// Display-only provenance: a cloud provider, an operator geofeed or a hosting
/// network is a fact the route card may state; a country estimate is not a
/// provider, and a city for an arbitrary hop is never claimed (the product
/// licenses no hop geolocation). This is the **one** caption/provider
/// implementation in the GUI — W2-4a retired the duplicate helper.
fn last_hop_provider(route: &ClientRoute) -> Option<String> {
    use lightspeed_client::app::location::{place, Place};
    let address = route.last_hop?;
    match place(address) {
        Place::Cloud { provider, .. } => Some(provider.to_ascii_uppercase()),
        Place::Geofeed { provider, .. } => Some(provider.to_string()),
        Place::Hosting { name, .. } => Some(name.to_string()),
        Place::Country { .. } | Place::Unknown => None,
    }
}

/// The taxonomy sentence for one error kind: one kind, one sentence (§2.8).
fn error_line(error: &lightspeed_client::app::dto::AppError) -> Line {
    let key = match error.kind {
        AppErrorKind::Capture | AppErrorKind::Tunnel => "error.tunnel",
        AppErrorKind::Route => "error.registration",
        AppErrorKind::Config => "error.config",
        AppErrorKind::Game | AppErrorKind::Validation => "error.game",
        AppErrorKind::Io => "error.io",
        AppErrorKind::Unsupported => "error.unsupported",
    };
    Line::new(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightspeed_client::app::dto::*;
    use lightspeed_client::EngineStatus;

    // ── Fixtures ──────────────────────────────────────────────────────────

    fn relay() -> std::net::SocketAddrV4 {
        "207.246.124.117:4434".parse().expect("relay address")
    }

    fn server() -> std::net::Ipv4Addr {
        std::net::Ipv4Addr::new(203, 0, 113, 7)
    }

    fn status() -> AppStatus {
        AppStatus {
            engine: EngineStatus {
                connected: true,
                proxy_addr: relay().to_string(),
                latest_rtt_ms: 305.0,
                ..EngineStatus::default()
            },
            interceptor: None,
            relay: RelayStatus {
                relay: Some(relay()),
                server: Some(server()),
                ..RelayStatus::default()
            },
            watchdog: WatchdogStatus::default(),
            location: LocationStatus::default(),
            server_leg: ServerLegStatus::default(),
            route_trace: None,
            boosting: false,
            relayed: None,
            error: None,
            session: SessionCommandState::default(),
            capabilities: Capabilities { elevated: true },
            tray: TrayState::Connected,
        }
    }

    fn boosting_status() -> AppStatus {
        let mut status = status();
        status.boosting = true;
        status.engine.interceptor_active = true;
        status.engine.tunnelling_enabled = Some(true);
        status.relayed = Some(RelayedPair {
            sent: 120,
            returned: 30,
        });
        status.tray = TrayState::Optimizing;
        status
    }

    fn warning_status(fault: ReturnPathFault) -> AppStatus {
        let mut status = boosting_status();
        let warning = StarvationWarning {
            server: std::net::SocketAddrV4::new(server(), 28015),
            relay: Some(relay()),
            fault,
        };
        status.watchdog = WatchdogStatus {
            warning: Some(warning),
            streak: None,
        };
        status.tray = TrayState::RelayStarved;
        status
    }

    // ── (b) contract rows, both directions ────────────────────────────────

    #[test]
    fn idle_has_two_words_from_the_connected_fact() {
        let mut disconnected = status();
        disconnected.engine.connected = false;
        disconnected.engine.proxy_addr = String::new();
        let surface = route_surface(&disconnected);
        assert_eq!(surface.word, RouteWord::NotConnected);
        assert_eq!(surface.word.key(), "state.disconnected");
        assert_eq!(
            surface.subline.as_ref().map(|line| line.key),
            Some("route.no_relay")
        );
        assert_eq!(surface.metric.value_ms, None, "no link, no metric");
        assert_eq!(surface.metric.caption_key, "route.leg_relay");

        let connected = route_surface(&status());
        assert_eq!(connected.word, RouteWord::Connected);
        assert_eq!(connected.word.key(), "state.connected");
        assert_eq!(connected.metric.value_ms, Some(305), "relay leg at display");
    }

    #[test]
    fn connecting_is_its_own_word_while_the_attempt_is_in_flight() {
        let mut connecting = status();
        connecting.engine.connected = false;
        connecting.engine.control_registered = false;
        connecting.relay.connecting = true;
        let surface = route_surface(&connecting);
        assert_eq!(surface.word, RouteWord::Connecting);
        assert_eq!(surface.word.key(), "state.connecting");
        let subline = surface.subline.expect("connecting names the relay");
        assert_eq!(subline.key, "route.connecting");
        assert_eq!(subline.args[0].0, "relay");

        let mut idle = connecting;
        idle.relay.connecting = false;
        assert_eq!(route_surface(&idle).word, RouteWord::NotConnected);
    }

    #[test]
    fn holding_is_the_engine_gate_and_choosing_is_its_sub_state() {
        let mut holding = boosting_status();
        holding.engine.tunnelling_enabled = Some(false);
        holding.relay.server = None;
        let surface = route_surface(&holding);
        assert_eq!(surface.word, RouteWord::Holding);
        assert_eq!(surface.word.key(), "state.holding");
        assert_eq!(
            surface.subline.as_ref().map(|line| line.key),
            Some("route.holding_scan"),
            "a hold with no locked server says it is still scanning"
        );

        holding.relay.server = Some(server());
        assert_eq!(
            route_surface(&holding)
                .subline
                .as_ref()
                .map(|line| line.key),
            Some("route.holding")
        );
        holding.relay.choosing = true;
        assert_eq!(
            route_surface(&holding)
                .subline
                .as_ref()
                .map(|line| line.key),
            Some("route.choosing"),
            "Choosing is a HOLDING sub-state, never its own word (A4)"
        );
        assert_eq!(route_surface(&holding).word, RouteWord::Holding);

        // The gate open is the only thing that leaves HOLDING.
        holding.engine.tunnelling_enabled = Some(true);
        assert_eq!(route_surface(&holding).word, RouteWord::Optimizing);
    }

    #[test]
    fn relaying_is_the_boosting_gate_and_shows_one_relayed_pair() {
        let surface = route_surface(&boosting_status());
        assert_eq!(surface.word, RouteWord::Optimizing);
        let relayed = surface
            .ledger
            .iter()
            .find(|row| row.label_key == "route.relayed")
            .expect("relayed row");
        assert!(
            matches!(&relayed.value, RowValue::Copy(line)
            if line.key == "route.sent_returned"
                && line.args
                    == vec![
                        ("sent", "120".to_string()),
                        ("returned", "30".to_string()),
                    ]),
            "the row states both counters from the one source pair: {:?}",
            relayed.value
        );

        let mut idle = boosting_status();
        idle.boosting = false;
        assert_ne!(route_surface(&idle).word, RouteWord::Optimizing);
    }

    #[test]
    fn a_starving_relayed_row_never_flips_the_word_without_the_watchdog() {
        let mut starving = boosting_status();
        starving.relayed = Some(RelayedPair {
            sent: 54,
            returned: 0,
        });
        let surface = route_surface(&starving);
        assert_eq!(
            surface.word,
            RouteWord::Optimizing,
            "A5/C3: the Relayed row warns, the word says what the boosting gate says"
        );
        assert!(surface.warning.is_none());
        let relayed = surface
            .ledger
            .iter()
            .find(|row| row.label_key == "route.relayed")
            .expect("relayed row");
        assert!(
            matches!(&relayed.value, RowValue::Warn(line)
            if line.key == "route.sent_returned"
                && line.args
                    == vec![
                        ("sent", "54".to_string()),
                        ("returned", "0".to_string()),
                    ]),
            "the warning still states both counters, including the zero: {:?}",
            relayed.value
        );

        // The one source pair decides the flag (W2-4b moved the retired
        // `relayed_row_text` rule here): a relay that answers is not starving,
        // and an idle pair has nothing to warn about.
        for (sent, returned) in [(54, 50), (0, 0)] {
            let mut answered = boosting_status();
            answered.relayed = Some(RelayedPair { sent, returned });
            let surface = route_surface(&answered);
            let relayed = surface
                .ledger
                .iter()
                .find(|row| row.label_key == "route.relayed")
                .expect("relayed row");
            assert!(
                matches!(&relayed.value, RowValue::Copy(line)
                if line.args
                    == vec![
                        ("sent", sent.to_string()),
                        ("returned", returned.to_string()),
                    ]),
                "{sent} sent / {returned} returned must render as data, not a warning: {:?}",
                relayed.value
            );
        }
    }

    #[test]
    fn no_return_path_outranks_optimizing_and_states_the_observation() {
        for (fault, key) in [
            (
                ReturnPathFault::NoReplyFromRelay,
                "route.relay_not_returning",
            ),
            (ReturnPathFault::TooFewReplies, "route.relay_not_returning"),
            (
                ReturnPathFault::InjectionFailed,
                "route.injection_failed_warning",
            ),
        ] {
            let surface = route_surface(&warning_status(fault));
            assert_eq!(surface.word, RouteWord::NoReturnPath);
            assert_eq!(surface.word.tone(), Tone::Danger);
            assert_eq!(surface.warning.as_ref().map(|line| line.key), Some(key));
            assert_eq!(surface.tray, TrayState::RelayStarved);
            // The route keeps being named: the player is still relayed (§2.2 r6).
            assert_eq!(
                surface.subline.as_ref().map(|line| line.key),
                Some("route.relaying_route")
            );
        }

        // A warning without a running session is not a word (warn-only).
        let mut idle = warning_status(ReturnPathFault::NoReplyFromRelay);
        idle.boosting = false;
        idle.engine.interceptor_active = false;
        assert_ne!(route_surface(&idle).word, RouteWord::NoReturnPath);
        assert!(route_surface(&idle).warning.is_none());
    }

    #[test]
    fn carry_and_streak_are_provenance_never_a_word() {
        let mut starved = boosting_status();
        starved.relay.carry = Some(CarryVerdict::Starved);
        assert_eq!(
            route_surface(&starved).word,
            RouteWord::Optimizing,
            "a starved carry record demotes the next ranking, it never moves the route (A7)"
        );

        // The streak alone (no confirmed warning) is still OPTIMIZING.
        let mut streaked = boosting_status();
        streaked.watchdog.streak = Some(1);
        assert_eq!(route_surface(&streaked).word, RouteWord::Optimizing);

        // A confirmed warning carries its evidence without changing the copy:
        // the fault picks the sentence, the streak is provenance.
        let mut confirmed = warning_status(ReturnPathFault::NoReplyFromRelay);
        confirmed.watchdog.streak = Some(2);
        let surface = route_surface(&confirmed);
        assert_eq!(surface.word, RouteWord::NoReturnPath);
        assert_eq!(
            surface.warning.as_ref().map(|line| line.key),
            Some("route.relay_not_returning")
        );
    }

    #[test]
    fn needs_elevation_is_an_overlay_and_never_a_word() {
        let mut unelevated = status();
        unelevated.capabilities = Capabilities { elevated: false };
        let surface = route_surface(&unelevated);
        assert_eq!(surface.overlay, Overlay::NeedsElevation);
        assert_eq!(
            surface.notice.as_ref().map(|line| line.key),
            Some("notice.needs_admin")
        );
        assert_eq!(surface.action.label_key, "action.restart_admin_optimize");
        assert_eq!(
            surface.word,
            RouteWord::Connected,
            "the rail keeps its route word under the overlay (A6)"
        );

        let mut elevated = unelevated;
        elevated.capabilities = Capabilities { elevated: true };
        assert_eq!(route_surface(&elevated).overlay, Overlay::None);
        assert_eq!(
            route_surface(&elevated).action.label_key,
            "action.optimize_route"
        );
    }

    #[test]
    fn the_elevation_invite_needs_a_boost_that_could_otherwise_start() {
        // R1/M1: unelevated alone is not an invitation. The invite exists only
        // when a relay is selected and the boost is otherwise ready.
        let mut unelevated = status();
        unelevated.capabilities = Capabilities { elevated: false };
        unelevated.engine.connected = false;
        unelevated.engine.proxy_addr = String::new();
        let surface = route_surface(&unelevated);
        assert_eq!(surface.overlay, Overlay::None, "no elevation overlay");
        assert_eq!(surface.notice, None, "no elevation notice");
        assert_eq!(
            surface.action.label_key, "action.optimize_route",
            "not the restart invite"
        );
        assert!(!surface.action.enabled);
        assert_eq!(
            surface.action.disabled_reason.as_ref().map(|line| line.key),
            Some("action_reason.not_connected")
        );

        // Disconnected but with no relay: still no invite; the missing relay is
        // the first unmet gate.
        unelevated.relay.relay = None;
        let surface = route_surface(&unelevated);
        assert_eq!(surface.overlay, Overlay::None);
        assert_eq!(surface.action.label_key, "action.optimize_route");
        assert_eq!(
            surface.action.disabled_reason.as_ref().map(|line| line.key),
            Some("action_reason.missing_relay")
        );

        // Connected, relay selected, unelevated: the invite is the answer.
        let mut ready_but_unelevated = status();
        ready_but_unelevated.capabilities = Capabilities { elevated: false };
        let surface = route_surface(&ready_but_unelevated);
        assert_eq!(surface.overlay, Overlay::NeedsElevation);
        assert_eq!(surface.action.label_key, "action.restart_admin_optimize");
        assert!(surface.action.enabled);
    }

    #[test]
    fn error_leads_the_word_and_keeps_its_details_behind_the_disclosure() {
        let mut failed = boosting_status();
        failed.error = Some(AppError::for_kind(AppErrorKind::Tunnel, "WSAEMSGSIZE"));
        failed.tray = TrayState::Error;
        let surface = route_surface(&failed);
        assert_eq!(surface.word, RouteWord::Error);
        assert_eq!(surface.word.key(), "state.error");
        // R7: the Error state's sub-line is the taxonomy sentence, not a route
        // sub-line left over from the previous state. The verbatim engine detail
        // stays on the error record the disclosure renders from: the seam never
        // folds it into the sentence (W2-4b deleted the duplicate accessor that
        // used to carry it).
        assert_eq!(
            surface.subline.as_ref().map(|line| line.key),
            Some("error.tunnel")
        );
        fn detail(status: &AppStatus) -> Option<String> {
            status.error.as_ref().and_then(|error| error.detail.clone())
        }
        assert_eq!(detail(&failed).as_deref(), Some("WSAEMSGSIZE"));

        let config = AppStatus {
            error: Some(AppError::for_kind(AppErrorKind::Config, "not toml [")),
            ..status()
        };
        assert_eq!(
            route_surface(&config).subline.as_ref().map(|line| line.key),
            Some("error.config")
        );
        assert_eq!(detail(&config).as_deref(), Some("not toml ["));

        // A healthy snapshot has no Error word and no error sentence.
        let healthy = status();
        assert_ne!(route_surface(&healthy).word, RouteWord::Error);
        assert!(detail(&healthy).is_none());
    }

    #[test]
    fn the_four_estimator_rows_are_the_fallback_when_the_relay_has_no_report() {
        use lightspeed_client::app::dto::MeasuredServerLeg;
        use std::time::Duration;
        let cases = [
            (
                ServerLegState::Measured(MeasuredServerLeg {
                    dest_leg_us: 40_000,
                    samples: 4,
                    age: Some(Duration::from_secs(3)),
                }),
                "route.server_leg_measured_age",
            ),
            (
                ServerLegState::Measured(MeasuredServerLeg {
                    dest_leg_us: 40_000,
                    samples: 4,
                    age: None,
                }),
                "route.server_leg_measured",
            ),
            (
                ServerLegState::Bounded { floor_us: 60_000 },
                "route.server_leg_bounded",
            ),
            (
                ServerLegState::Prior { prior_us: 41_000 },
                "route.server_leg_prior",
            ),
        ];
        for (state, key) in cases {
            let mut status = boosting_status();
            status.relay.server_leg = state;
            let surface = route_surface(&status);
            let row = surface
                .details
                .iter()
                .find(|row| row.label_key == "route.server_leg")
                .expect("estimator fallback row");
            match &row.value {
                RowValue::Copy(line) => assert_eq!(line.key, key),
                other => panic!("expected copy row for {state:?}, got {other:?}"),
            }
        }
        let mut unknown = boosting_status();
        unknown.relay.server_leg = ServerLegState::Unknown;
        let surface = route_surface(&unknown);
        let row = surface
            .details
            .iter()
            .find(|row| row.label_key == "route.server_leg")
            .expect("unknown row");
        assert_eq!(row.value, RowValue::Hint("route.server_leg_unknown"));

        // Not relaying: no leg row at all (nothing carries traffic).
        let mut idle = boosting_status();
        idle.boosting = false;
        idle.engine.interceptor_active = false;
        idle.relay.server_leg = ServerLegState::Prior { prior_us: 41_000 };
        assert!(
            route_surface(&idle)
                .details
                .iter()
                .all(|row| row.label_key != "route.server_leg"),
            "the estimator row appears only while that route carries traffic"
        );
    }

    #[test]
    fn relay_measured_rows_lead_and_the_estimator_falls_back() {
        let mut measured = boosting_status();
        measured.server_leg.icmp = Some(lightspeed_client::ServerLegReport {
            median_us: 41_000,
            jitter_us: 2_000,
            samples: 4,
            age: std::time::Duration::from_secs(3),
            validated: true,
        });
        measured.server_leg.expected_path_ms = Some(61.0);
        measured.server_leg.app = Some(lightspeed_client::ServerLegAppReport {
            passive_median_us: 47_000,
            samples: 9,
            age: std::time::Duration::from_secs(2),
            floor_ok: true,
        });
        measured.server_leg.trace = Some(lightspeed_client::ServerTraceReport {
            hops: 9,
            last_hop: None,
            last_hop_rtt_us: Some(12_000),
            complete: true,
            age: std::time::Duration::from_secs(4),
        });
        measured.relay.server_leg = ServerLegState::Prior { prior_us: 41_000 };
        let surface = route_surface(&measured);
        let labels: Vec<&str> = surface.details.iter().map(|row| row.label_key).collect();
        for label in [
            "route.expected_path_label",
            "route.server_leg_app",
            "route.route_trace",
        ] {
            assert!(
                labels.contains(&label),
                "missing relay-measured row {label}"
            );
        }
        // M15: the label key is not the value template and carries no
        // placeholder; the value is the template with its ms arg.
        let expected = surface
            .details
            .iter()
            .find(|row| row.label_key == "route.expected_path_label")
            .expect("expected path row");
        assert!(
            !expected.label_key.contains('{'),
            "a label must never render a placeholder template"
        );
        assert!(
            matches!(&expected.value, RowValue::Copy(line)
                if line.key == "route.expected_path"
                    && line.args == vec![("ms", "61".to_string())]),
            "the value is the expected-path template with its ms arg: {:?}",
            expected.value
        );

        // W2-4b moved the retired GUI copy builders' rules into this fixture: the
        // seam states the same measurement, sample count and age, and the rows
        // that are data only (passive app RTT, trace, expected path) claim
        // exactly the terms the shared status carries.
        let copy_line = |surface: &RouteSurface, label: &str| -> Line {
            let row = surface
                .details
                .iter()
                .find(|row| row.label_key == label)
                .unwrap_or_else(|| panic!("missing details row {label}"));
            match &row.value {
                RowValue::Copy(line) => line.clone(),
                other => panic!("{label} must render catalogued copy, got {other:?}"),
            }
        };
        let leg_row = surface
            .ledger
            .iter()
            .find(|row| row.label_key == "route.server_leg")
            .expect("measured leg is the default row");
        assert!(
            matches!(&leg_row.value, RowValue::Copy(line)
            if line.key == "route.server_leg_relay_measured"
                && line.args
                    == vec![
                        ("ms", "41".to_string()),
                        ("n", "4".to_string()),
                        ("secs", "3".to_string()),
                    ]),
            "the measured leg states ms, n and age: {:?}",
            leg_row.value
        );
        // A report accepted fail-open must not present as validated: the key
        // says so, and the number with its n stays the relay's measurement.
        let mut fail_open = measured.clone();
        if let Some(report) = fail_open.server_leg.icmp.as_mut() {
            report.validated = false;
        }
        let fail_open_leg = route_surface(&fail_open)
            .ledger
            .into_iter()
            .find(|row| row.label_key == "route.server_leg")
            .expect("fail-open measured leg");
        assert!(
            matches!(&fail_open_leg.value, RowValue::Copy(line)
            if line.key == "route.server_leg_relay_measured_unvalidated"
                && line.args
                    == vec![
                        ("ms", "41".to_string()),
                        ("n", "4".to_string()),
                        ("secs", "3".to_string()),
                    ]),
            "a fail-open report says measured, unvalidated: {:?}",
            fail_open_leg.value
        );

        let app = copy_line(&surface, "route.server_leg_app");
        assert_eq!(app.key, "route.server_leg_app_measured");
        assert_eq!(
            app.args,
            vec![("ms", "47".to_string()), ("n", "9".to_string())],
            "the passive app row states the median and its sample count, no age"
        );
        // The below-floor flag is harvest data, not a different number or a
        // hidden row: the same figure still renders.
        let mut flagged = measured.clone();
        if let Some(report) = flagged.server_leg.app.as_mut() {
            report.floor_ok = false;
        }
        assert_eq!(
            copy_line(&route_surface(&flagged), "route.server_leg_app"),
            app,
            "an invalid passive sample is flagged by the client, not re-formed here"
        );

        let trace = copy_line(&surface, "route.route_trace");
        assert_eq!(trace.key, "route.route_trace_hops");
        assert_eq!(
            trace.args,
            vec![("hops", "9".to_string()), ("ms", "12".to_string())],
            "the trace row states hops and the last router RTT"
        );
        // A trace with no responding router still states the hops it measured,
        // never a fabricated anchor.
        let mut no_router = measured.clone();
        no_router.server_leg.trace = Some(lightspeed_client::ServerTraceReport {
            hops: 7,
            last_hop: None,
            last_hop_rtt_us: None,
            complete: false,
            age: std::time::Duration::from_secs(1),
        });
        let no_router_trace = copy_line(&route_surface(&no_router), "route.route_trace");
        assert_eq!(no_router_trace.key, "route.route_trace_hops_only");
        assert_eq!(no_router_trace.args, vec![("hops", "7".to_string())]);

        // No displayed client leg means no path total: `S` alone is not a path.
        // (The zero/NaN arithmetic itself is shared core and tested there.)
        let mut no_total = measured.clone();
        no_total.server_leg.expected_path_ms = None;
        assert!(
            !route_surface(&no_total)
                .details
                .iter()
                .any(|row| row.label_key == "route.expected_path_label"),
            "no client leg, no Expected path row"
        );

        // The best Server leg row is on the default surface; the estimator
        // fallback is not rendered when the relay has a report.
        assert!(
            surface
                .ledger
                .iter()
                .any(|row| row.label_key == "route.server_leg"),
            "the measured leg is the default row"
        );
        assert!(
            !labels.contains(&"route.server_leg"),
            "the estimator fallback does not duplicate the measured leg"
        );

        // R3: a trace report without an ICMP leg report is a reachable state;
        // the estimator fallback (keyed on the *report*) must still be shown.
        let mut trace_only = boosting_status();
        trace_only.relay.server_leg = ServerLegState::Prior { prior_us: 41_000 };
        trace_only.server_leg.trace = Some(lightspeed_client::ServerTraceReport {
            hops: 9,
            last_hop: None,
            last_hop_rtt_us: Some(12_000),
            complete: true,
            age: std::time::Duration::from_secs(4),
        });
        let surface = route_surface(&trace_only);
        let labels: Vec<&str> = surface.details.iter().map(|row| row.label_key).collect();
        assert!(labels.contains(&"route.route_trace"), "the trace row");
        assert!(
            labels.contains(&"route.server_leg"),
            "the estimator fallback is keyed on report absence, not trace absence"
        );

        // No report and no trace: still the estimator fallback, no fabricated
        // measured row.
        let plain = route_surface(&boosting_status());
        let labels: Vec<&str> = plain.details.iter().map(|row| row.label_key).collect();
        assert!(labels.contains(&"route.server_leg"));
        assert!(!labels.contains(&"route.expected_path_label"));
        assert!(!plain
            .ledger
            .iter()
            .any(|row| row.label_key == "route.server_leg"));
    }

    // ── Rendered copy: rules moved from the retired copy builders (W2-4b) ──

    /// The shell renders a seam [`Line`] through `i18n::t_with` (`app.rs`
    /// `line_text`) and the action label through `i18n::t(..).to_uppercase()`.
    /// These assertions take the seam's own output and render it exactly that
    /// way, so the user-visible sentences the W2-4b deletion retired stay pinned
    /// where the rows are built — no second caption implementation returns.
    #[test]
    fn the_rendered_rows_state_the_copy_rules_of_the_retired_helpers() {
        use std::time::Duration;

        fn rendered(line: &Line) -> String {
            let args: Vec<(&str, &str)> = line
                .args
                .iter()
                .map(|(key, value)| (*key, value.as_str()))
                .collect();
            crate::i18n::t_with(line.key, &args)
        }
        fn copy_line(surface: &RouteSurface, label: &str) -> Line {
            let row = surface
                .details
                .iter()
                .find(|row| row.label_key == label)
                .unwrap_or_else(|| panic!("missing details row {label}"));
            match &row.value {
                RowValue::Copy(line) => line.clone(),
                other => panic!("{label} must render catalogued copy, got {other:?}"),
            }
        }

        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(Some("en"));

        let report = lightspeed_client::ServerLegReport {
            median_us: 148_000,
            jitter_us: 3_000,
            samples: 4,
            age: Duration::from_secs(7),
            validated: true,
        };
        let mut measured = boosting_status();
        measured.server_leg.icmp = Some(report);
        // L (90 ms, the Relay RTT the card shows) + S (148 ms) = 238 ms.
        measured.server_leg.expected_path_ms = Some(238.0);
        measured.server_leg.app = Some(lightspeed_client::ServerLegAppReport {
            passive_median_us: 47_250,
            samples: 9,
            age: Duration::from_secs(2),
            floor_ok: true,
        });
        measured.server_leg.trace = Some(lightspeed_client::ServerTraceReport {
            hops: 12,
            last_hop: Some(std::net::Ipv4Addr::new(8, 8, 8, 8)),
            last_hop_rtt_us: Some(14_250),
            complete: false,
            age: Duration::from_secs(3),
        });
        let surface = route_surface(&measured);

        // The retired `relay_measured_leg_text`: median, n, age; fail-open says
        // so instead of presenting the figure as validated.
        let leg = surface
            .ledger
            .iter()
            .find(|row| row.label_key == "route.server_leg")
            .expect("measured leg");
        match &leg.value {
            RowValue::Copy(line) => assert_eq!(
                rendered(line),
                "Relay→server 148 ms · measured n=4 · 7s ago"
            ),
            other => panic!("the measured leg is copy, got {other:?}"),
        }
        let unvalidated = lightspeed_client::ServerLegReport {
            validated: false,
            ..report
        };
        let mut fail_open = measured.clone();
        fail_open.server_leg.icmp = Some(unvalidated);
        let fail_open_leg = route_surface(&fail_open)
            .ledger
            .into_iter()
            .find(|row| row.label_key == "route.server_leg")
            .expect("fail-open leg");
        match &fail_open_leg.value {
            RowValue::Copy(line) => assert_eq!(
                rendered(line),
                "Relay→server 148 ms · measured, unvalidated n=4 · 7s ago"
            ),
            other => panic!("the fail-open leg is copy, got {other:?}"),
        }

        // The retired `relay_measured_app_leg_text`: median and sample count.
        assert_eq!(
            rendered(&copy_line(&surface, "route.server_leg_app")),
            "47 ms · n=9"
        );

        // The retired `relay_measured_trace_text`: hops + last router RTT; a
        // trace with no router answer still reports the measured distance.
        assert_eq!(
            rendered(&copy_line(&surface, "route.route_trace")),
            "12 hops · last router RTT 14 ms"
        );
        let mut no_router = measured.clone();
        no_router.server_leg.trace = Some(lightspeed_client::ServerTraceReport {
            hops: 7,
            last_hop: None,
            last_hop_rtt_us: None,
            complete: false,
            age: Duration::from_secs(1),
        });
        assert_eq!(
            rendered(&copy_line(&route_surface(&no_router), "route.route_trace")),
            "7 hops · no router answered"
        );

        // The retired `expected_path_text`: the §2.7 B form, no placeholder
        // leak (M15).
        assert_eq!(
            rendered(&copy_line(&surface, "route.expected_path_label")),
            "~238 ms (relay leg + server leg)"
        );

        // The retired `relayed_row_text`: both counters, and the starving pair is
        // the warning branch of the same one source pair.
        let mut starving = boosting_status();
        starving.relayed = Some(RelayedPair {
            sent: 54,
            returned: 0,
        });
        let relayed = route_surface(&starving)
            .ledger
            .into_iter()
            .find(|row| row.label_key == "route.relayed")
            .expect("relayed row");
        match &relayed.value {
            RowValue::Warn(line) => assert_eq!(rendered(line), "54 up · 0 down"),
            other => panic!("the starving pair is the warning branch, got {other:?}"),
        }
        let mut answered = starving.clone();
        answered.relayed = Some(RelayedPair {
            sent: 54,
            returned: 50,
        });
        let relayed = route_surface(&answered)
            .ledger
            .into_iter()
            .find(|row| row.label_key == "route.relayed")
            .expect("relayed row");
        match &relayed.value {
            RowValue::Copy(line) => assert_eq!(rendered(line), "54 up · 50 down"),
            other => panic!("an answered pair is data, got {other:?}"),
        }

        // The retired `primary_action`: the four gate labels, drawn in caps by the
        // shell exactly like this assertion.
        let caps = |label_key: &str| crate::i18n::t(label_key).to_uppercase();
        assert_eq!(
            caps(route_surface(&status()).action.label_key),
            "OPTIMIZE MY ROUTE"
        );
        assert_eq!(
            caps(route_surface(&boosting_status()).action.label_key),
            "STOP OPTIMIZING"
        );
        let mut unelevated = status();
        unelevated.capabilities = Capabilities { elevated: false };
        assert_eq!(
            caps(route_surface(&unelevated).action.label_key),
            "RESTART AS ADMINISTRATOR TO OPTIMIZE"
        );
        let mut not_ready = unelevated;
        not_ready.engine.connected = false;
        assert_eq!(
            caps(route_surface(&not_ready).action.label_key),
            "OPTIMIZE MY ROUTE"
        );

        crate::i18n::set_language(None);
    }

    #[test]
    fn the_location_ladder_maps_each_rung_and_draws_no_dot_it_cannot_justify() {
        let cloud = LocationStatus {
            rung: LocationRung::Cloud,
            provider: Some("gcore"),
            region: Some("us-east-1"),
            chooser_point: Some((38.0, -77.0)),
            ..LocationStatus::default()
        };
        let mut status = boosting_status();
        status.location = cloud;
        let block = route_surface(&status).location.expect("cloud rung");
        assert_eq!(block.rung.key, "route.location_provider");
        assert_eq!(block.hint, None);
        assert!(
            matches!(&block.detail, Some(RowValue::Data(value)) if value == "GCORE · us-east-1"),
            "the cloud detail is the provider region: {:?}",
            block.detail
        );

        let geofeed = LocationStatus {
            rung: LocationRung::Geofeed,
            geofeed: Some(GeofeedProvenance {
                provider: "G-Core",
                country: "US",
                region: Some("US-VA"),
                city: Some("Manassas"),
                source: "https://example/geofeed.csv",
                as_of: "2026-11-19",
            }),
            ..LocationStatus::default()
        };
        status.location = geofeed;
        let block = route_surface(&status).location.expect("geofeed rung");
        assert_eq!(
            block.rung.key, "route.location_geofeed",
            "the rung caption key"
        );
        assert_eq!(
            block.rung.args,
            vec![("place", "Manassas, US-VA".to_string())],
            "the {{place}} template is supplied, never emitted bare"
        );
        assert!(
            matches!(&block.detail, Some(RowValue::Copy(line)) if line.key == "route.location_geofeed_detail"),
            "the provider/as-of detail line"
        );
        assert_eq!(
            block.as_is,
            Some("route.location_geofeed_note"),
            "a geofeed is shown as-is"
        );
        assert!(block.source.is_some(), "the source URL is the hover fact");

        // A hosting rung without both facts cannot state its network: the block
        // shows the honest unknown instead of `AS0` or a blank name (R11).
        let mut incomplete = status.clone();
        incomplete.location = LocationStatus {
            rung: LocationRung::Hosting,
            asn: Some(0),
            hosting: None,
            ..LocationStatus::default()
        };
        let block = route_surface(&incomplete)
            .location
            .expect("incomplete hosting rung");
        assert_eq!(block.rung.key, "route.location_unknown");
        assert_eq!(block.detail, None, "never AS0 / a blank network name");

        let hosting = LocationStatus {
            rung: LocationRung::Hosting,
            asn: Some(20_473),
            hosting: Some("DigitalOcean"),
            ..LocationStatus::default()
        };
        status.location = hosting;
        let block = route_surface(&status).location.expect("hosting rung");
        assert_eq!(block.rung.key, "route.location_hosting");
        assert_eq!(block.hint, Some("route.location_hosting_hint"));
        assert!(
            matches!(&block.detail, Some(RowValue::Data(value)) if value == "AS20473 · DigitalOcean"),
            "the hosting detail: {:?}",
            block.detail
        );

        status.location = LocationStatus {
            rung: LocationRung::Country,
            country: Some("JP"),
            ..LocationStatus::default()
        };
        assert_eq!(
            route_surface(&status)
                .location
                .expect("country rung")
                .rung
                .key,
            "route.location_estimate"
        );
        status.location = LocationStatus {
            rung: LocationRung::Unknown,
            ..LocationStatus::default()
        };
        assert_eq!(
            route_surface(&status)
                .location
                .expect("unknown rung")
                .rung
                .key,
            "route.location_unknown"
        );

        // No server, no block.
        let mut no_server = boosting_status();
        no_server.relay.server = None;
        assert!(route_surface(&no_server).location.is_none());
    }

    #[test]
    fn the_route_trace_is_a_caption_never_a_ping() {
        let mut traced = boosting_status();
        traced.route_trace = Some(ClientRoute {
            hops: 7,
            last_hop: None,
            last_hop_rtt_ms: Some(31),
            complete: true,
        });
        let trace = route_surface(&traced).trace.expect("trace caption");
        assert_eq!(
            trace.caption.key, "route.trace_route",
            "the shipped caption key carries the hops"
        );
        assert_eq!(trace.caption.args, vec![("hops", "7".to_string())]);
        let nearest = trace.nearest.expect("a router answered");
        assert_eq!(nearest.key, "route.trace_nearest");
        assert_eq!(nearest.args, vec![("ms", "31".to_string())]);
        assert_eq!(
            trace.provider, None,
            "no last-hop address: no provider label is invented"
        );

        let no_router = route_surface(&AppStatus {
            route_trace: Some(ClientRoute {
                hops: 5,
                last_hop: None,
                last_hop_rtt_ms: None,
                complete: false,
            }),
            ..boosting_status()
        });
        let trace = no_router.trace.expect("trace caption");
        assert_eq!(trace.caption.args, vec![("hops", "5".to_string())]);
        assert_eq!(trace.nearest, None, "no router answered: no suffix");

        assert!(route_surface(&boosting_status()).trace.is_none());
    }

    /// A trace whose last responding hop is in a shipped location index states
    /// the provider/hosting network name — never a city, never a country
    /// estimate (W2-4a wired the suffix into this one caption implementation).
    #[test]
    fn the_trace_provider_is_a_shipped_index_fact_never_a_city() {
        fn traced(hop: &str) -> AppStatus {
            let mut status = boosting_status();
            status.route_trace = Some(ClientRoute {
                hops: 9,
                last_hop: Some(hop.parse().expect("hop address")),
                last_hop_rtt_ms: Some(31),
                complete: false,
            });
            status
        }

        // The AWS cloud range: the provider is a stated fact, uppercased.
        let cloud = route_surface(&traced("3.237.221.191"))
            .trace
            .expect("trace row");
        assert_eq!(cloud.provider.as_deref(), Some("AWS"));
        assert_eq!(cloud.caption.key, "route.trace_route");
        assert!(
            !cloud.provider.as_deref().unwrap().contains(' '),
            "the suffix is one network name, not a place: {:?}",
            cloud.provider
        );

        // The live G-Core hosting address: the operator's name, not the
        // geofeed city that places the server rather than an arbitrary hop.
        let hosting = route_surface(&traced("178.107.14.201"))
            .trace
            .expect("trace row");
        assert_eq!(hosting.provider.as_deref(), Some("G-Core"));

        // A documentation/unknown range: no provider label is invented.
        let unknown = route_surface(&traced("203.0.113.9"))
            .trace
            .expect("trace row");
        assert_eq!(unknown.provider, None);
    }

    /// A trace with no responding hop (timeout / 0 hops) is no row at all: the
    /// caption would otherwise claim a measurement the probe never made.
    #[test]
    fn a_trace_without_a_responding_hop_is_no_row() {
        let timeout = AppStatus {
            route_trace: Some(ClientRoute {
                hops: 0,
                last_hop: None,
                last_hop_rtt_ms: None,
                complete: false,
            }),
            ..boosting_status()
        };
        assert!(route_surface(&timeout).trace.is_none());
    }

    #[test]
    fn the_tray_never_says_optimizing_while_the_warning_is_active() {
        let surface = route_surface(&warning_status(ReturnPathFault::NoReplyFromRelay));
        assert_eq!(surface.tray, TrayState::RelayStarved);
        assert_ne!(surface.tray, TrayState::Optimizing);

        let healthy = route_surface(&boosting_status());
        assert_eq!(healthy.tray, TrayState::Optimizing);

        let mut failed = boosting_status();
        failed.error = Some(AppError::for_kind(AppErrorKind::Tunnel, "boom"));
        failed.tray = TrayState::Error;
        assert_eq!(route_surface(&failed).tray, TrayState::Error);
    }

    #[test]
    fn the_action_slot_is_gate_driven_and_never_claims_it_can_act() {
        let mut blocked = status();
        blocked.engine.connected = false;
        blocked.relay.relay = None;
        let action = route_surface(&blocked).action;
        assert!(!action.enabled);
        assert_eq!(
            action.disabled_reason.as_ref().map(|line| line.key),
            Some("action_reason.missing_relay"),
            "a control that cannot act says why (M6)"
        );

        let mut connecting = blocked;
        connecting.relay.relay = Some(relay());
        connecting.relay.connecting = true;
        assert_eq!(
            route_surface(&connecting)
                .action
                .disabled_reason
                .as_ref()
                .map(|line| line.key),
            Some("action_reason.waiting_relay")
        );

        // STARTING… is a real in-flight state and cannot re-trigger.
        let mut starting = boosting_status();
        starting.session.in_flight = Some(SessionCommand::StartBoost);
        let action = route_surface(&starting).action;
        assert_eq!(action.label_key, "action.starting");
        assert!(!action.enabled);
        assert!(action.in_flight);

        // W2-4b moved the retired `primary_action` copy rule here: the four gate
        // outcomes map to the catalogued labels, and only the boosting slot is
        // the quiet destructive control.
        let ready = route_surface(&status()).action;
        assert_eq!(ready.label_key, "action.optimize_route");
        assert!(ready.enabled);
        assert!(!ready.danger);

        let stop = route_surface(&boosting_status()).action;
        assert_eq!(stop.label_key, "action.stop_optimizing");
        assert!(stop.enabled);
        assert!(
            stop.danger,
            "the stop action is the quiet destructive control, never the bright accent"
        );

        let mut unelevated = status();
        unelevated.capabilities = Capabilities { elevated: false };
        let invite = route_surface(&unelevated).action;
        assert_eq!(invite.label_key, "action.restart_admin_optimize");
        assert!(invite.enabled);

        // M1: an unelevated app that cannot boost is not invited to elevate; the
        // slot stays the ordinary disabled start.
        let mut unelevated_not_ready = unelevated;
        unelevated_not_ready.engine.connected = false;
        let blocked_invite = route_surface(&unelevated_not_ready).action;
        assert_eq!(blocked_invite.label_key, "action.optimize_route");
        assert!(!blocked_invite.enabled);
    }

    // ── (d) Relay-recovered absence + WatchdogPlan transitions ─────────────

    #[test]
    fn the_default_ledger_shows_the_rows_the_contract_names() {
        // Connected idle: Relay, Relay RTT (measured), Game server, Relayed.
        let surface = route_surface(&status());
        let labels: Vec<&str> = surface.ledger.iter().map(|row| row.label_key).collect();
        assert_eq!(
            labels,
            vec![
                "route.relay",
                "route.relay_rtt",
                "route.game_server",
                "route.relayed"
            ],
            "R5: Relay RTT is a default row when measured"
        );
        assert_eq!(surface.ledger[0].value, RowValue::Data(relay().to_string()));
        assert_eq!(
            surface.ledger[1].value,
            RowValue::Data("305 ms".to_string())
        );
        assert_eq!(
            surface.ledger[2].value,
            RowValue::Data(server().to_string())
        );
        assert_eq!(
            surface.ledger[3].value,
            RowValue::Hint("route.count_hint"),
            "no session counters yet"
        );

        // No measurement, no Relay RTT row.
        let mut no_measurement = status();
        no_measurement.engine.latest_rtt_ms = 0.0;
        assert!(!route_surface(&no_measurement)
            .ledger
            .iter()
            .any(|row| row.label_key == "route.relay_rtt"));

        // Relaying: the same rows plus the one session pair.
        let relaying = route_surface(&boosting_status());
        let labels: Vec<&str> = relaying.ledger.iter().map(|row| row.label_key).collect();
        assert_eq!(
            labels,
            vec![
                "route.relay",
                "route.relay_rtt",
                "route.game_server",
                "route.relayed"
            ]
        );
        assert!(matches!(
            relaying.ledger[3].value,
            RowValue::Copy(ref line) if line.key == "route.sent_returned"
        ));

        // Holding: Path says Holding and Relayed says nothing has been relayed
        // yet (spec §2.2 row 3 / §3), even though the pair is Some(0, 0).
        let mut holding = boosting_status();
        holding.engine.tunnelling_enabled = Some(false);
        holding.relayed = Some(RelayedPair {
            sent: 0,
            returned: 0,
        });
        let surface = route_surface(&holding);
        let labels: Vec<&str> = surface.ledger.iter().map(|row| row.label_key).collect();
        assert!(labels.contains(&"route.path"), "the Path row while held");
        let path = surface
            .ledger
            .iter()
            .find(|row| row.label_key == "route.path")
            .expect("path row");
        assert!(matches!(&path.value, RowValue::Warn(line) if line.key == "state.holding"));
        let relayed = surface
            .ledger
            .iter()
            .find(|row| row.label_key == "route.relayed")
            .expect("relayed row");
        assert_eq!(
            relayed.value,
            RowValue::Hint("route.relayed_hold"),
            "R6: the hold says no traffic relayed yet, not 0 up · 0 down"
        );

        // Disconnected: Relay is a placeholder, never the stale address.
        let mut idle = status();
        idle.engine.connected = false;
        idle.engine.proxy_addr = String::new();
        let relay_row = &route_surface(&idle).ledger[0];
        assert_eq!(relay_row.value, RowValue::Hint("--"));
    }

    #[test]
    fn the_details_include_the_direct_ping_before_a_session() {
        // R15: the direct ping is not tunnel-scoped; a connected idle window
        // that measured it still shows the row (in Details).
        let mut pinged = status();
        pinged.relay.server_ping = Some(ServerPing {
            server: server(),
            ms: 283.0,
            age_secs: 4,
        });
        let surface = route_surface(&pinged);
        let labels: Vec<&str> = surface.details.iter().map(|row| row.label_key).collect();
        assert_eq!(labels, vec!["route.server_ping"]);
        assert!(matches!(
            &surface.details[0].value,
            RowValue::Data(value) if value == "283 ms · 4s"
        ));

        // No ping measured: no row.
        assert!(route_surface(&status()).details.is_empty());
    }

    #[test]
    fn the_cannot_beat_direct_row_states_the_arithmetic() {
        // R8: the shipped caption has no placeholders, so the two provable
        // measurements are emitted as data; the caption is the label only.
        let mut status = boosting_status();
        status.relay.cannot_beat_direct = Some((305_840, 283_474));
        let surface = route_surface(&status);
        let row = surface
            .details
            .iter()
            .find(|row| row.label_key == "route.cannot_beat_direct")
            .expect("the triangle caption row");
        assert!(
            !row.label_key.contains('{'),
            "the caption is not a template"
        );
        assert!(
            matches!(&row.value, RowValue::Data(value)
                if value.contains("305") && value.contains("283")),
            "the arithmetic carries both measurements: {:?}",
            row.value
        );
        assert!(!route_surface(&boosting_status())
            .details
            .iter()
            .any(|row| row.label_key == "route.cannot_beat_direct"));
    }

    #[test]
    fn the_idle_subline_names_the_game_mode() {
        // The shipped short forms: pick-your-game with no game, named when the
        // engine has one. (The §2.2 "Auto waits / follows / pinned" wording is
        // a W2-2 copy decision; the fact path is the same field.)
        assert_eq!(
            route_surface(&status()).subline.map(|line| line.key),
            Some("route.relay_ready_pick")
        );
        let mut named = status();
        named.engine.redirect_game = "Rust".to_string();
        let subline = route_surface(&named).subline.expect("named sub-line");
        assert_eq!(subline.key, "route.relay_ready_named");
        assert_eq!(subline.args, vec![("name", "Rust".to_string())]);
    }

    #[test]
    fn disabled_reasons_follow_the_gate_order() {
        let mut registration = status();
        registration.engine.connected = false;
        registration.engine.registration_error = Some("relay refused".to_string());
        assert_eq!(
            route_surface(&registration)
                .action
                .disabled_reason
                .as_ref()
                .map(|line| line.key),
            Some("action_reason.registration")
        );

        let mut not_connected = status();
        not_connected.engine.connected = false;
        assert_eq!(
            route_surface(&not_connected)
                .action
                .disabled_reason
                .as_ref()
                .map(|line| line.key),
            Some("action_reason.not_connected")
        );
    }

    #[test]
    fn there_is_no_recovered_word_or_banner() {
        // Exhaustive over the word set: no `Recovered`/`Direct`/`Retesting`
        // variant exists (A7 retired them), and the adapter never sets the
        // banner flag even after a raised-then-cleared warning.
        let words = [
            RouteWord::NotConnected,
            RouteWord::Connected,
            RouteWord::Connecting,
            RouteWord::Holding,
            RouteWord::Optimizing,
            RouteWord::NoReturnPath,
            RouteWord::Error,
        ];
        for word in words {
            assert!(!word.key().contains("recover"), "no recovered word");
        }
        let mut cleared = warning_status(ReturnPathFault::NoReplyFromRelay);
        cleared.watchdog.warning = None;
        cleared.watchdog.streak = None;
        cleared.tray = TrayState::Optimizing;
        let surface = route_surface(&cleared);
        assert_eq!(surface.word, RouteWord::Optimizing);
        assert!(!surface.relay_recovered);
        assert!(surface.warning.is_none());
    }

    #[test]
    fn watchdog_plan_fixtures_raise_then_clear_the_warning() {
        use crate::app::watchdog::{plan_return_path_window, RelayReplyFault, ReplyFaultStreak};
        use std::net::SocketAddrV4;

        let server_addr = SocketAddrV4::new(server(), 28015);
        let mut streak = ReplyFaultStreak::default();
        let mut current: Option<crate::app::watchdog::RelayStarvationWarning> = None;

        // Window 1: one fault, not yet confirmed → still OPTIMIZING.
        let confirmed = streak.observe(Some(RelayReplyFault::NoReplyFromRelay));
        let plan = plan_return_path_window(
            current.as_ref(),
            confirmed,
            Some(RelayReplyFault::NoReplyFromRelay),
            server_addr,
            Some(relay()),
        );
        current = plan.warning;
        let mut status = boosting_status();
        status.watchdog.warning = map_warning(current);
        assert_eq!(route_surface(&status).word, RouteWord::Optimizing);

        // Window 2: the streak threshold confirms the same fault → warning.
        let confirmed = streak.observe(Some(RelayReplyFault::NoReplyFromRelay));
        assert_eq!(
            confirmed,
            Some(RelayReplyFault::NoReplyFromRelay),
            "the plan fixture must reach the confirm threshold"
        );
        let plan = plan_return_path_window(
            current.as_ref(),
            confirmed,
            Some(RelayReplyFault::NoReplyFromRelay),
            server_addr,
            Some(relay()),
        );
        current = plan.warning;
        assert!(!plan.switch_to_direct, "warn-only: the plan never switches");
        status.watchdog.warning = map_warning(current);
        assert_eq!(route_surface(&status).word, RouteWord::NoReturnPath);

        // Window 3: a fully healthy window clears it; no banner is invented.
        let confirmed = streak.observe(None);
        let plan = plan_return_path_window(current.as_ref(), confirmed, None, server_addr, None);
        current = plan.warning;
        status.watchdog.warning = map_warning(current);
        let surface = route_surface(&status);
        assert_eq!(surface.word, RouteWord::Optimizing);
        assert!(surface.warning.is_none());
        assert!(!surface.relay_recovered);
    }

    fn map_warning(
        warning: Option<crate::app::watchdog::RelayStarvationWarning>,
    ) -> Option<StarvationWarning> {
        warning.map(|warning| StarvationWarning {
            server: warning.server,
            relay: warning.relay,
            fault: match warning.fault {
                crate::app::watchdog::RelayReplyFault::NoReplyFromRelay => {
                    ReturnPathFault::NoReplyFromRelay
                }
                crate::app::watchdog::RelayReplyFault::InjectionFailed => {
                    ReturnPathFault::InjectionFailed
                }
                crate::app::watchdog::RelayReplyFault::TooFewReplies => {
                    ReturnPathFault::TooFewReplies
                }
            },
        })
    }
}
