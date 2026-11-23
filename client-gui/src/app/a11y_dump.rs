//! `lightspeed-gui --a11y-dump <path>`: the window's accessibility tree as JSON.
//!
//! This is the offline, deterministic half of the accessibility contract. It
//! renders the *real* window shell and every sheet through
//! [`super::LightSpeedApp::render_ui`] with no window, no tray, no network and
//! no saved user configuration, then writes the AccessKit tree the Windows
//! adapter exposes to UI Automation (UIA). Agents and CI use it to assert the
//! control tree without a desktop session or a UIA client;
//! `scripts/uia-tree.ps1` exercises the live path.
//!
//! The dump deliberately contains **sample state** for the connected screens:
//! RFC 5737 TEST-NET relay/server addresses, no user profiles, no machine
//! paths. It is a schema of the UI, not a record of this machine.

use super::a11y::AxTree;
use super::*;
use crate::platform::{CurrentPlatform, InstallChannel};
use crate::update::ReleaseAsset;
use std::path::Path;

/// The JSON schema version of a dump. Bump only on a breaking change.
const SCHEMA: u64 = 1;

/// The window size the dump renders at: the product's contract width.
const DUMP_WINDOW: (f32, f32) = (320.0, 760.0);

/// Write the dump to `path` and return the JSON value that was written.
pub(crate) fn run(path: &Path) -> anyhow::Result<serde_json::Value> {
    let value = dump_value()?;
    let text = serde_json::to_string_pretty(&value)?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, text)?;
    // A GUI-subsystem process has no console, but a redirected stdout is valid:
    // `lightspeed-gui --a11y-dump out.json > log` still reports where it went.
    println!("{}", path.display());
    Ok(value)
}

/// Build the dump value without touching the filesystem.
///
/// Crate-visible so the accessibility tests can assert the exact JSON an agent
/// driver consumes.
pub(crate) fn dump_value() -> anyhow::Result<serde_json::Value> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let engine = Arc::new(Mutex::new(LightSpeedEngine::new(runtime.handle().clone())));
    let quit: QuitFlag = Arc::new(AtomicBool::new(false));
    let mut app = LightSpeedApp::<CurrentPlatform>::new_offline(engine, quit);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let geometry = WindowGeometry {
        monitor_height: Some(1080.0),
        window_top: Some(0.0),
    };

    let mut screens: Vec<(&str, AxTree)> = Vec::new();
    screens.push(("main", render_screen(&ctx, &mut app, geometry)));
    seed_connected_sample(&mut app);
    screens.push(("main_connected", render_screen(&ctx, &mut app, geometry)));
    seed_starved_sample(&mut app);
    screens.push(("main_starved", render_screen(&ctx, &mut app, geometry)));
    seed_error_sample(&mut app);
    screens.push(("main_error", render_screen(&ctx, &mut app, geometry)));
    seed_notice_sample(&mut app);
    screens.push(("main_notice", render_screen(&ctx, &mut app, geometry)));
    seed_manual_sample(&mut app);
    screens.push(("main_manual", render_screen(&ctx, &mut app, geometry)));
    seed_target_sample(&mut app);
    screens.push(("main_target", render_screen(&ctx, &mut app, geometry)));
    reset_sample(&mut app);

    app.show_settings = true;
    screens.push(("settings", render_screen(&ctx, &mut app, geometry)));
    app.show_settings = false;

    app.game_editor = GameEditorDraft::new_profile();
    // An existing draft is the only state that offers Delete; seed one so the
    // state-dependent action id is in the dump too.
    app.game_editor.editing = Some(0);
    app.game_editor.name = "Sample game".to_string();
    app.game_editor.process_names = "sample-game.exe".to_string();
    app.game_editor.udp_range = "36700-36800".to_string();
    app.process_list = ProcessListState::Done;
    app.show_game_editor = true;
    screens.push(("game_editor", render_screen(&ctx, &mut app, geometry)));
    app.show_game_editor = false;

    app.process_list = ProcessListState::Done;
    app.process_rows = vec![ProcessEntry {
        pid: 1234,
        name: "sample-game.exe".to_string(),
        path: Some("C:\\Games\\sample-game\\sample-game.exe".to_string()),
        started_at: None,
    }];
    app.show_target_picker = true;
    screens.push(("target_picker", render_screen(&ctx, &mut app, geometry)));
    app.show_target_picker = false;

    app.show_update_dialog = true;
    // An available in-place update is the only state that offers Install; seed
    // it so the conditional action id is in the dump too.
    app.update_check = UpdateCheckState::Done(Ok(UpdateStatus {
        current: env!("CARGO_PKG_VERSION").to_string(),
        latest: Some("9.9.9".to_string()),
        update_available: true,
        installer: Some(ReleaseAsset {
            tag: "v9.9.9".to_string(),
            zip_url: "https://example.invalid/lightspeed-test.zip".to_string(),
            sha256_url: "https://example.invalid/lightspeed-test.zip.sha256".to_string(),
        }),
        channel: InstallChannel::WindowsInPlace,
    }));
    screens.push(("update", render_screen(&ctx, &mut app, geometry)));
    app.show_update_dialog = false;
    app.update_check = UpdateCheckState::Idle;

    let screens_json: Vec<serde_json::Value> = screens
        .iter()
        .map(|(name, tree)| {
            serde_json::json!({
                "name": name,
                "tree": tree_json(tree),
            })
        })
        .collect();

    // The runtime must outlive the engine use above; drop order is explicit so a
    // panic in rendering cannot leave the engine pointing at a dead runtime.
    drop(app);
    drop(runtime);

    Ok(serde_json::json!({
        "schema": SCHEMA,
        "app": "lightspeed-gui",
        "version": env!("CARGO_PKG_VERSION"),
        "language": "en",
        "window": { "width": DUMP_WINDOW.0, "height": DUMP_WINDOW.1 },
        "state": "offline sample: RFC 5737 TEST-NET addresses, no user data",
        "screens": screens_json,
    }))
}

/// One screen's tree, with the real shell rendered at the product's window
/// size. Three frames: the first installs fonts/theme and the second settles
/// layout (egui uses the previous pass's widget rects), the third is the tree
/// that is captured.
pub(super) fn render_screen(
    ctx: &egui::Context,
    app: &mut LightSpeedApp<CurrentPlatform>,
    geometry: WindowGeometry,
) -> AxTree {
    ctx.enable_accesskit();
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(DUMP_WINDOW.0, DUMP_WINDOW.1),
        )),
        ..Default::default()
    };
    let mut tree = None;
    for _ in 0..3 {
        let mut output = ctx.run_ui(raw.clone(), |ui| app.render_ui(ui, geometry));
        output.textures_delta.clear();
        tree = Some(AxTree::from_output(&output));
    }
    tree.expect("at least one rendered frame")
}

/// A representative connected session, so the route surface and ledger are real
/// nodes rather than empty placeholders. Every address is RFC 5737 TEST-NET.
pub(super) fn seed_connected_sample(app: &mut LightSpeedApp<CurrentPlatform>) {
    let relay: std::net::SocketAddrV4 = "192.0.2.10:4434".parse().expect("TEST-NET relay");
    let relay_alt: std::net::SocketAddrV4 = "198.51.100.23:4434".parse().expect("TEST-NET relay");
    let server: std::net::SocketAddrV4 = "203.0.113.9:36704".parse().expect("TEST-NET server");
    app.proxies = vec![
        ProxyEntry {
            addr: relay,
            label: "TEST-NET Relay A".to_string(),
            node_id: Some("relay-test-a".to_string()),
        },
        ProxyEntry {
            addr: relay_alt,
            label: "TEST-NET Relay B".to_string(),
            node_id: Some("relay-test-b".to_string()),
        },
    ];
    app.auto_select = false;
    app.selected_proxy_idx = 0;
    app.status.connected = true;
    app.status.proxy_addr = relay.to_string();
    app.status.node_id = Some("relay-test-a".to_string());
    app.status.latest_rtt_ms = 42.0;
    app.status.rtt_history = vec![48.0, 45.0, 44.0, 43.0, 42.0, 42.0, 41.0, 42.0];
    app.status.interceptor_active = true;
    app.status.interceptor_platform = "sample";
    app.status.interceptor_server = server.to_string();
    app.status.interceptor_intercepted = 1_204;
    app.status.interceptor_tunnel_sent = 1_198;
    app.status.interceptor_from_proxy = 1_180;
    app.status.interceptor_injected = 1_180;
    app.status.interceptor_seen = 1_204;
    app.holding = false;
    app.watchdog = WatchdogSnapshot::default();
    app.starvation_warning = None;
}

/// The starvation warning: the one state where the shell must not claim
/// optimization. Seeded on top of the connected sample.
pub(super) fn seed_starved_sample(app: &mut LightSpeedApp<CurrentPlatform>) {
    let warning = StarvationWarning {
        server: "203.0.113.9:36704".parse().expect("TEST-NET server"),
        relay: Some("192.0.2.10:4434".parse().expect("TEST-NET relay")),
        fault: ReturnPathFault::NoReplyFromRelay,
    };
    app.watchdog = WatchdogSnapshot {
        warning: Some(warning),
        streak: Some(3),
        verdict: None,
    };
    app.starvation_warning = Some(warning);
}

/// A command failure: the one state that renders the assertive error panel and
/// the `main.error.panel` / open-log ids.
pub(super) fn seed_error_sample(app: &mut LightSpeedApp<CurrentPlatform>) {
    reset_sample(app);
    app.status.interceptor_error = Some("sample tunnel failure (TEST-NET fixture)".to_string());
}

/// A declined-elevation notice: the warn notice above the config region.
pub(super) fn seed_notice_sample(app: &mut LightSpeedApp<CurrentPlatform>) {
    reset_sample(app);
    app.elevation_notice = Some("notice.elevation_declined");
}

/// The Manual/Custom flow: its five fields and the process-target front door.
pub(super) fn seed_manual_sample(app: &mut LightSpeedApp<CurrentPlatform>) {
    reset_sample(app);
    app.game_manual = true;
    app.game_auto = false;
    app.server_input = "203.0.113.9:36704".to_string();
    app.custom_port_input = "36700-36800".to_string();
    app.save_server_name = "Sample server".to_string();
}

/// A picked process target: the state that offers Clear instead of Pick.
pub(super) fn seed_target_sample(app: &mut LightSpeedApp<CurrentPlatform>) {
    reset_sample(app);
    app.target_process = Some(ProcessTarget::by_name("sample-game.exe"));
}

/// Back to the offline idle state for the sheet screens.
pub(super) fn reset_sample(app: &mut LightSpeedApp<CurrentPlatform>) {
    app.proxies.clear();
    app.auto_select = true;
    app.selected_proxy_idx = 0;
    app.status = EngineStatus::default();
    app.holding = false;
    app.watchdog = WatchdogSnapshot::default();
    app.starvation_warning = None;
}

/// Serialize one tree into the dump's node list. `children`/`labelled_by` hold
/// node ids, so a reader can rebuild the tree (or look nodes up by
/// `automation_id`).
fn tree_json(tree: &AxTree) -> serde_json::Value {
    let nodes: Vec<serde_json::Value> = tree
        .nodes
        .iter()
        .map(|node| {
            serde_json::json!({
                "id": node.id,
                "role": format!("{:?}", node.role),
                "name": node.label,
                "value": node.value,
                "description": node.description,
                "automation_id": node.automation_id,
                "live": node.live.map(|live| format!("{live:?}")),
                "live_atomic": node.live_atomic,
                "expanded": node.expanded,
                "toggled": node.toggled.map(|toggled| format!("{toggled:?}")),
                "disabled": node.disabled,
                "focusable": node.focusable,
                "clickable": node.clickable,
                "labelled_by": node.labelled_by,
                "children": node.children,
            })
        })
        .collect();
    serde_json::json!({
        "root": tree.root,
        "focus": tree.focus,
        "nodes": nodes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dump is a contract for agents and CI: every screen must exist and
    /// every screen must carry a non-empty tree.
    #[test]
    fn the_dump_covers_every_screen() {
        let value = dump_value().expect("the offline dump renders");
        assert_eq!(value["schema"], SCHEMA);
        assert_eq!(value["language"], "en");
        let screens = value["screens"].as_array().expect("screens array");
        let names: Vec<&str> = screens
            .iter()
            .map(|screen| screen["name"].as_str().expect("screen name"))
            .collect();
        assert_eq!(
            names,
            vec![
                "main",
                "main_connected",
                "main_starved",
                "main_error",
                "main_notice",
                "main_manual",
                "main_target",
                "settings",
                "game_editor",
                "target_picker",
                "update"
            ]
        );
        for screen in screens {
            let nodes = screen["tree"]["nodes"].as_array().expect("node list");
            assert!(
                nodes.len() > 3,
                "{} rendered no accessibility nodes",
                screen["name"]
            );
            for node in nodes {
                assert!(
                    node["role"].as_str().is_some_and(|role| !role.is_empty()),
                    "a node in {} has no role",
                    screen["name"]
                );
            }
        }
    }

    /// Inspection helper: writes the full dump under the test temp dir so a
    /// developer can read the exact tree. Ignored in the normal gate.
    #[test]
    #[ignore = "inspection helper; run explicitly to write the dump"]
    fn write_dump_for_inspection() {
        let path = std::env::temp_dir().join("v1125-dump.json");
        let value = run(&path).expect("the offline dump renders");
        assert!(value["screens"].is_array());
    }

    /// The dump must not become a host report: the offline app hides the
    /// machine's user profiles, servers and profile keys from every screen.
    #[test]
    fn the_offline_renderer_hides_local_user_data() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a current-thread runtime");
        let engine = Arc::new(Mutex::new(LightSpeedEngine::new(runtime.handle().clone())));
        let quit: QuitFlag = Arc::new(AtomicBool::new(false));
        let app = LightSpeedApp::<CurrentPlatform>::new_offline(engine, quit);
        assert!(app.offline, "the offline constructor must mark the app");
        assert!(app.view_user_profiles().is_empty());
        assert!(app.view_user_servers().is_empty());
        assert!(app.view_user_game_keys().is_empty());
    }

    /// Every automation id whose control only exists in a non-default state is
    /// seeded by at least one screen, so an agent can discover it offline. The
    /// two deliberate absences are pinned: the tray-only Hide entry and the
    /// saved-servers combo (offline hides local user data).
    #[test]
    fn every_state_dependent_automation_id_is_seeded() {
        let value = dump_value().expect("the offline dump renders");
        let mut ids = std::collections::HashSet::new();
        for screen in value["screens"].as_array().expect("screens") {
            for node in screen["tree"]["nodes"].as_array().expect("nodes") {
                if let Some(id) = node["automation_id"].as_str() {
                    ids.insert(id.to_string());
                }
            }
        }
        for expected in [
            "main.error.panel",
            "main.error.details",
            "main.notice",
            "main.manual.server",
            "main.manual.port_range",
            "main.manual.save_name",
            "main.manual.save",
            "main.target.process",
            "main.target.clear",
            "editor.button.delete",
            "update.button.install",
        ] {
            assert!(
                ids.contains(expected),
                "{expected} is not seeded by any screen"
            );
        }
        assert!(
            !ids.contains("main.footer.hide"),
            "Hide to tray only exists with a real tray; it is live-verified instead"
        );
        assert!(
            !ids.contains("main.manual.saved_servers"),
            "the saved-servers combo is hidden offline because it is user data"
        );
    }
}
