//! # Accessibility tree support
//!
//! The GUI is an immediate-mode egui app, and egui already writes an
//! [AccessKit](https://accesskit.dev/) tree for every widget that reports
//! [`egui::WidgetInfo`]. On Windows that tree *is* the standard UI Automation
//! (UIA) tree: a screen reader, Windows Narrator, a magnifier, or an automation
//! driver (PowerShell + `UIAutomationClient`, WinAppDriver, Playwright-for-UIA)
//! finds the app's controls through it.
//!
//! Two gaps remain, and this module closes both:
//!
//! 1. **Custom-painted widgets report nothing.** A `Sense::click()` rectangle
//!    with painted text gets only a bare "unknown" node; painted text gets no
//!    node at all. Screen readers cannot name it and automation cannot find it.
//!    [`AccessKitResponse`] is the one place a response is given a role, a
//!    name, a description, an automation id, or a live-region priority.
//! 2. **Automation needs stable handles, not visible text.** Visible text is
//!    localized and changes with state. [`automation`] holds the stable
//!    `AutomationId` strings (UIA's `AutomationId` property, from AccessKit's
//!    `author_id`) that tests and agent drivers target. They are a contract:
//!    renaming one is a breaking change for automation.
//!
//! ## Live regions
//!
//! Screen readers do not read a painted value that changes under the cursor.
//! The state word, the starvation warning and the error lines are marked as
//! AccessKit live regions with [`AccessKitResponse::a11y_live`]: AccessKit's
//! Windows adapter raises `UIA_LiveRegionChanged` when the node's name changes,
//! which is how a screen reader learns that the route started optimizing.
//!
//! ## Testing
//!
//! [`AxTree`] is the whole tree as a testable value. It is built from the
//! frame's [`egui::PlatformOutput::accesskit_update`] with no window, no UIA
//! client, and no screen reader, so the accessibility surface is asserted in
//! the normal `cargo test -p lightspeed-gui` gate. `lightspeed-gui
//! --a11y-dump <path>` uses the same collector to write the tree as JSON for
//! agent drivers and CI.

use eframe::egui;
use egui::accesskit;
use egui::{Response, WidgetInfo};

/// Stable `AutomationId` values for the controls automation drives.
///
/// AccessKit's Windows adapter exposes [`accesskit::Node::author_id`] as UIA's
/// `AutomationId`, so these strings survive localization and layout changes.
/// The namespace is dotted: `<screen>.<region>.<control>`. Tests and agent
/// drivers must target these, never visible labels; the labels are localized.
pub(crate) mod automation {
    // ── Main shell — status instrument ───────────────────────────────────
    pub(crate) const STATE_WORD: &str = "main.state.word";
    pub(crate) const STATE_DETAIL: &str = "main.state.detail";
    pub(crate) const ROUND_TRIP: &str = "main.state.metric";
    pub(crate) const ROUTE_MAP: &str = "main.route.map";
    pub(crate) const ROUTE_DETAILS: &str = "main.route.details";
    pub(crate) const ERROR_DETAILS: &str = "main.error.details";
    /// Prefix for a data-rail row; append the row's catalogue `label_key`.
    pub(crate) const ROUTE_ROW: &str = "main.route.row.";
    pub(crate) const LOCATION_ROW: &str = "main.route.location";
    pub(crate) const WARNING: &str = "main.warning.starvation";
    pub(crate) const ERROR_PANEL: &str = "main.error.panel";
    pub(crate) const NOTICE: &str = "main.notice";
    pub(crate) const ACTION_PRIMARY: &str = "main.action.primary";
    pub(crate) const ACTION_REASON: &str = "main.action.reason";

    // ── Main shell — controls ────────────────────────────────────────────
    pub(crate) const RELAY_SELECT: &str = "main.relay.select";
    pub(crate) const RELAY_DISCONNECT: &str = "main.relay.disconnect";
    pub(crate) const RELAY_RETRY: &str = "main.relay.retry";
    pub(crate) const GAME_SELECT: &str = "main.game.select";
    pub(crate) const GAME_DETECTED: &str = "main.game.detected";
    pub(crate) const MANUAL_SERVER: &str = "main.manual.server";
    pub(crate) const MANUAL_PORT_RANGE: &str = "main.manual.port_range";
    pub(crate) const MANUAL_SAVE_NAME: &str = "main.manual.save_name";
    pub(crate) const MANUAL_SAVE: &str = "main.manual.save";
    pub(crate) const MANUAL_SAVED_SERVERS: &str = "main.manual.saved_servers";
    pub(crate) const TARGET_PROCESS: &str = "main.target.process";
    pub(crate) const TARGET_CLEAR: &str = "main.target.clear";
    pub(crate) const FOOTER_HIDE: &str = "main.footer.hide";
    pub(crate) const FOOTER_SETTINGS: &str = "main.footer.settings";
    pub(crate) const FOOTER_QUIT: &str = "main.footer.quit";

    // ── Settings sheet ───────────────────────────────────────────────────
    pub(crate) const SETTINGS_SHEET: &str = "settings.sheet";
    pub(crate) const SETTINGS_BUTTON_CLOSE: &str = "settings.button.close";
    pub(crate) const SETTINGS_LANGUAGE: &str = "settings.language.select";
    pub(crate) const SETTINGS_LOG: &str = "settings.button.open_log";
    pub(crate) const SETTINGS_CHECK_UPDATES: &str = "settings.button.check_updates";
    pub(crate) const SETTINGS_SHARE_STATS: &str = "settings.checkbox.share_stats";
    pub(crate) const SETTINGS_PROXY_LABEL: &str = "settings.proxy.label";
    pub(crate) const SETTINGS_PROXY_ADDR: &str = "settings.proxy.addr";
    pub(crate) const SETTINGS_PROXY_ADD: &str = "settings.proxy.add";
    pub(crate) const SETTINGS_PROXY_RESET: &str = "settings.proxy.reset";
    pub(crate) const SETTINGS_REMEASURE: &str = "settings.button.remeasure";
    pub(crate) const SETTINGS_REFRESH_RELAYS: &str = "settings.button.refresh_relays";

    // ── Update sheet ─────────────────────────────────────────────────────
    pub(crate) const UPDATE_SHEET: &str = "update.sheet";
    pub(crate) const UPDATE_BUTTON_CLOSE: &str = "update.button.close";
    pub(crate) const UPDATE_BUTTON_INSTALL: &str = "update.button.install";

    // ── Game-editor sheet ────────────────────────────────────────────────
    pub(crate) const EDITOR_SHEET: &str = "editor.sheet";
    pub(crate) const EDITOR_BUTTON_CLOSE: &str = "editor.button.close";
    pub(crate) const EDITOR_BUTTON_SAVE: &str = "editor.button.save";
    pub(crate) const EDITOR_BUTTON_DELETE: &str = "editor.button.delete";
    pub(crate) const EDITOR_FIELD_NAME: &str = "editor.field.name";
    pub(crate) const EDITOR_FIELD_PROCESSES: &str = "editor.field.processes";
    pub(crate) const EDITOR_FIELD_UDP_RANGE: &str = "editor.field.udp_range";
    pub(crate) const EDITOR_FIELD_NOTES: &str = "editor.field.notes";
    pub(crate) const EDITOR_DYNAMIC_SERVER: &str = "editor.checkbox.dynamic_server";

    // ── Process picker sheet ─────────────────────────────────────────────
    pub(crate) const PICKER_SHEET: &str = "picker.sheet";
    pub(crate) const PICKER_BUTTON_CLOSE: &str = "picker.button.close";
    pub(crate) const PICKER_SEARCH: &str = "picker.search";
    pub(crate) const PICKER_REFRESH: &str = "picker.button.refresh";
}
/// Give an [`egui::Response`] the accessibility metadata the UI intends.
///
/// Every method is a no-op when AccessKit is not active for this frame (no
/// screen reader and no UIA client), so calling them is cheap and always safe.
pub(crate) trait AccessKitResponse {
    /// Report this widget with a role, an enabled state and an accessible
    /// name. Replaces the default node's "unknown" role.
    fn a11y_widget(self, role: accesskit::Role, enabled: bool, name: impl Into<String>)
        -> Response;

    /// Report a label/value pair in the ledger style: the name is the value,
    /// because a plain [`accesskit::Role::Label`] carries its text in `value`
    /// (`WidgetInfo`'s own rule).
    fn a11y_text(self, text: impl Into<String>) -> Response;

    /// Give the node a stable UIA `AutomationId`.
    fn a11y_automation_id(self, id: &str) -> Response;

    /// Add a screen-reader-only description (provenance, units, a hint) without
    /// creating a visible tooltip. Kept for surfaces that need more than a name.
    #[allow(dead_code)]
    fn a11y_description(self, text: impl Into<String>) -> Response;

    /// Mark the node as a live region: screen readers announce name changes
    /// at this priority. `atomic` announces the whole node rather than only
    /// the changed text.
    fn a11y_live(self, live: accesskit::Live, atomic: bool) -> Response;

    /// Hide a purely decorative node (a brand image, a spinner with an adjacent
    /// text label) from the accessibility tree.
    fn a11y_hidden(self) -> Response;

    /// Report whether a disclosure control is expanded and advertise the
    /// AccessKit Expand/Collapse actions a screen reader can use on it.
    fn a11y_disclosure(self, expanded: bool) -> Response;

    /// Report this visible line as an assertive live alert, with `text` as its
    /// accessible name.
    ///
    /// AccessKit's Windows adapter announces a live region by its *name*; a
    /// [`accesskit::Role::Label`] carries its text in `value`, while
    /// [`accesskit::Role::Alert`] needs `label`. This helper sets both
    /// correctly, so a screen reader actually reads the fault.
    fn a11y_alert(self, text: impl Into<String>) -> Response;
}

impl AccessKitResponse for Response {
    fn a11y_widget(
        self,
        role: accesskit::Role,
        enabled: bool,
        name: impl Into<String>,
    ) -> Response {
        let name = name.into();
        self.widget_info(|| WidgetInfo::labeled(role, enabled, &name));
        self
    }

    fn a11y_text(self, text: impl Into<String>) -> Response {
        self.a11y_widget(accesskit::Role::Label, true, text)
    }

    fn a11y_automation_id(self, id: &str) -> Response {
        self.ctx
            .accesskit_node_builder(self.id, |node| node.set_author_id(id));
        self
    }

    #[allow(dead_code)]
    fn a11y_description(self, text: impl Into<String>) -> Response {
        let text = text.into();
        self.ctx
            .accesskit_node_builder(self.id, |node| node.set_description(text));
        self
    }

    fn a11y_live(self, live: accesskit::Live, atomic: bool) -> Response {
        self.ctx.accesskit_node_builder(self.id, |node| {
            node.set_live(live);
            if atomic {
                node.set_live_atomic();
            }
        });
        self
    }

    fn a11y_hidden(self) -> Response {
        self.ctx
            .accesskit_node_builder(self.id, |node| node.set_hidden());
        self
    }

    fn a11y_disclosure(self, expanded: bool) -> Response {
        self.ctx.accesskit_node_builder(self.id, |node| {
            node.set_expanded(expanded);
            // egui does not turn these actions into clicks itself; the
            // disclosure helper consumes them (`disclosure_action`).
            node.add_action(accesskit::Action::Expand);
            node.add_action(accesskit::Action::Collapse);
        });
        self
    }

    fn a11y_alert(self, text: impl Into<String>) -> Response {
        let text = text.into();
        self.widget_info(|| WidgetInfo::labeled(accesskit::Role::Alert, true, &text));
        self.ctx.accesskit_node_builder(self.id, |node| {
            node.set_live(accesskit::Live::Assertive);
            node.set_live_atomic();
        });
        self
    }
}

// ── Tree snapshot ────────────────────────────────────────────────────────
//
// The collector below is the testable form of what eframe hands to AccessKit's
// platform adapter. It exists so the accessibility surface is covered by the
// crate's own tests, and (via `--a11y-dump`) by agent drivers that cannot
// attach a UIA client.

/// One node of the frame's accessibility tree, flattened for assertions.
#[derive(Clone, Debug)]
pub(super) struct AxNode {
    /// AccessKit node id, stable within a frame and derivable from the egui id.
    pub(super) id: String,
    /// The raw id, for tests that inject AccessKit action requests.
    #[cfg(test)]
    pub(super) accesskit_id: accesskit::NodeId,
    pub(super) role: accesskit::Role,
    /// UIA `Name`: the label, or empty when the node is named through
    /// `labelled_by` (see [`Self::has_name`]).
    pub(super) label: Option<String>,
    /// The text AccessKit's `Label`/`Definition` roles carry in `value`.
    pub(super) value: Option<String>,
    pub(super) description: Option<String>,
    /// UIA `AutomationId`, from [`accesskit::Node::author_id`].
    pub(super) automation_id: Option<String>,
    pub(super) live: Option<accesskit::Live>,
    pub(super) live_atomic: bool,
    pub(super) expanded: bool,
    /// Checked/pressed state for a toggle role (`None` when the role has no
    /// toggled state).
    pub(super) toggled: Option<accesskit::Toggled>,
    pub(super) disabled: bool,
    pub(super) focusable: bool,
    pub(super) clickable: bool,
    pub(super) labelled_by: Vec<String>,
    pub(super) children: Vec<String>,
}

impl AxNode {
    /// Whether the node has an accessible name, directly or by label
    /// association. An input-role node without one cannot be found by a
    /// screen reader or by a test that looks it up by name.
    #[cfg(test)]
    pub(super) fn has_name(&self) -> bool {
        self.label.as_deref().is_some_and(|label| !label.is_empty()) || !self.labelled_by.is_empty()
    }

    /// The visible text a screen reader would read for a data node.
    #[cfg(test)]
    pub(super) fn text(&self) -> &str {
        self.label
            .as_deref()
            .or(self.value.as_deref())
            .unwrap_or_default()
    }
}

/// The frame's accessibility tree: the root node plus every node, flattened.
#[derive(Clone, Debug)]
pub(super) struct AxTree {
    pub(super) root: String,
    pub(super) focus: String,
    pub(super) nodes: Vec<AxNode>,
}

impl AxTree {
    /// Build the tree from one rendered frame.
    ///
    /// Panics when AccessKit produced no update: every test that uses this
    /// helper calls [`egui::Context::enable_accesskit`] first, and a silently
    /// missing tree is exactly the regression these tests exist to catch.
    pub(super) fn from_output(output: &egui::FullOutput) -> Self {
        let update = output.platform_output.accesskit_update.as_ref().expect(
            "AccessKit produced no tree update; call ctx.enable_accesskit() before the frame",
        );
        let nodes = update
            .nodes
            .iter()
            .map(|(id, node)| AxNode {
                id: format!("{id:?}"),
                #[cfg(test)]
                accesskit_id: *id,
                role: node.role(),
                label: node.label().map(str::to_owned),
                value: node.value().map(str::to_owned),
                description: node.description().map(str::to_owned),
                automation_id: node.author_id().map(str::to_owned),
                live: node.live(),
                live_atomic: node.is_live_atomic(),
                expanded: node.is_expanded().unwrap_or(false),
                toggled: node.toggled(),
                disabled: node.is_disabled(),
                focusable: node.supports_action(accesskit::Action::Focus),
                clickable: node.supports_action(accesskit::Action::Click),
                labelled_by: node
                    .labelled_by()
                    .iter()
                    .map(|id| format!("{id:?}"))
                    .collect(),
                children: node.children().iter().map(|id| format!("{id:?}")).collect(),
            })
            .collect();
        Self {
            root: format!("{:?}", update.tree.as_ref().map(|tree| tree.root)),
            focus: format!("{:?}", update.focus),
            nodes,
        }
    }

    /// The nodes that fill an input role, in tree order. Every one must have
    /// an accessible name: that is the invariant test.
    #[cfg(test)]
    pub(super) fn inputs(&self) -> Vec<&AxNode> {
        self.nodes
            .iter()
            .filter(|node| egui::accessibility::is_input(node.role))
            .collect()
    }

    /// Find a node by its stable automation id.
    #[cfg(test)]
    pub(super) fn by_automation_id(&self, id: &str) -> Option<&AxNode> {
        self.nodes
            .iter()
            .find(|node| node.automation_id.as_deref() == Some(id))
    }

    /// Find the first node whose visible text (label or value) contains
    /// `needle`. Test-only; automation code uses ids.
    #[cfg(test)]
    pub(super) fn containing_text(&self, needle: &str) -> Option<&AxNode> {
        self.nodes.iter().find(|node| node.text().contains(needle))
    }

    /// Input-role nodes that no screen reader can name.
    #[cfg(test)]
    pub(super) fn unnamed_inputs(&self) -> Vec<&AxNode> {
        self.inputs()
            .into_iter()
            .filter(|node| !node.has_name())
            .collect()
    }

    /// Assert the "every input is named" rule, with a readable failure.
    #[cfg(test)]
    pub(super) fn assert_every_input_is_named(&self) {
        let unnamed: Vec<String> = self
            .unnamed_inputs()
            .into_iter()
            .map(|node| format!("{:?} {:?}", node.role, node.text()))
            .collect();
        assert!(
            unnamed.is_empty(),
            "input-role accessibility nodes without a name: {unnamed:?}"
        );
    }

    /// Duplicate automation ids: two nodes with one id make automation
    /// ambiguous, so the id set is part of the contract.
    #[cfg(test)]
    pub(super) fn duplicate_automation_ids(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut duplicates = Vec::new();
        for id in self
            .nodes
            .iter()
            .filter_map(|node| node.automation_id.as_deref())
        {
            if !seen.insert(id) {
                duplicates.push(id.to_string());
            }
        }
        duplicates
    }
}

/// Read and consume a disclosure's AccessKit Expand/Collapse action.
///
/// egui 0.37 consumes scroll and text actions but drops Expand/Collapse, so a
/// screen reader's `IExpandCollapseProvider` was inert. Returns the requested
/// state (`Some(true)` expand, `Some(false)` collapse) and removes the event so
/// the widget does not see it as a normal click.
pub(crate) fn disclosure_action(ui: &egui::Ui, id: egui::Id) -> Option<bool> {
    let mut requested = None;
    ui.ctx().input_mut(|input| {
        input.consume_accesskit_action_requests(id, |request| match request.action {
            accesskit::Action::Expand => {
                requested = Some(true);
                true
            }
            accesskit::Action::Collapse => {
                requested = Some(false);
                true
            }
            _ => false,
        });
    });
    requested
}
#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{self, accesskit, vec2, Id, Sense, WidgetInfo};

    /// Render two frames (fonts and widget rects settle on the second) with
    /// AccessKit enabled, and return the resulting tree.
    fn render(mut build: impl FnMut(&mut egui::Ui)) -> AxTree {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                vec2(420.0, 800.0),
            )),
            ..Default::default()
        };
        // The first pass is thrown away: UI state (and the accesskit node set)
        // is complete on the second pass. Texture deltas must be consumed or
        // epaint panics on drop.
        let mut first = ctx.run_ui(raw.clone(), |ui| build(ui));
        first.textures_delta.clear();
        let mut output = ctx.run_ui(raw, |ui| build(ui));
        output.textures_delta.clear();
        AxTree::from_output(&output)
    }

    #[test]
    fn a_standard_widget_is_named_and_actionable() {
        let tree = render(|ui| {
            let _ = ui.button("Standard button");
        });
        let button = tree
            .containing_text("Standard button")
            .expect("the standard button is in the tree");
        assert_eq!(button.role, accesskit::Role::Button);
        assert!(button.clickable, "a button must expose the Click action");
        assert!(button.has_name(), "a button must have an accessible name");
        tree.assert_every_input_is_named();
    }

    #[test]
    fn a_custom_painted_widget_reports_role_name_and_action() {
        let tree = render(|ui| {
            let response = ui.allocate_exact_size(vec2(120.0, 40.0), Sense::click()).1;
            response
                .a11y_widget(accesskit::Role::Button, true, "Custom painted button")
                .a11y_automation_id("test.custom_button")
                .a11y_description("A button the app paints itself");
        });
        let node = tree
            .by_automation_id("test.custom_button")
            .expect("the custom button has its automation id");
        assert_eq!(node.role, accesskit::Role::Button);
        assert_eq!(node.label.as_deref(), Some("Custom painted button"));
        assert_eq!(
            node.description.as_deref(),
            Some("A button the app paints itself")
        );
        assert!(node.clickable, "a custom button must expose Click");
        tree.assert_every_input_is_named();
    }

    #[test]
    fn a_painted_text_node_carries_the_text_screen_readers_read() {
        let tree = render(|ui| {
            let response = ui.allocate_exact_size(vec2(80.0, 20.0), Sense::hover()).1;
            response
                .a11y_text("Relayed 54 · 0")
                .a11y_automation_id("test.painted_text");
        });
        let node = tree
            .by_automation_id("test.painted_text")
            .expect("the painted text node exists");
        assert_eq!(node.role, accesskit::Role::Label);
        assert_eq!(node.value.as_deref(), Some("Relayed 54 · 0"));
        assert_eq!(node.text(), "Relayed 54 · 0");
    }

    #[test]
    fn a_modal_is_part_of_the_same_tree() {
        // The project's own docs once said egui popups were invisible to
        // accessibility tooling. AccessKit 0.37 puts stacks in one tree; this
        // test is the proof, and it guards the sheet work that depends on it.
        let tree = render(|ui| {
            let ctx = ui.ctx().clone();
            egui::Modal::new(Id::unique("test_modal")).show(&ctx, |ui| {
                ui.label("Sheet body");
                let _ = ui.button("Sheet close");
            });
        });
        assert!(
            tree.containing_text("Sheet body").is_some(),
            "modal content is missing from the accessibility tree"
        );
        assert!(
            tree.containing_text("Sheet close").is_some(),
            "modal controls are missing from the accessibility tree"
        );
        let has_dialog = tree
            .nodes
            .iter()
            .any(|node| node.role == accesskit::Role::Dialog);
        assert!(has_dialog, "the modal stack must be a Dialog node");
        tree.assert_every_input_is_named();
    }

    #[test]
    fn a_live_region_is_marked_for_announcement() {
        let tree = render(|ui| {
            let response = ui.label("CONNECTED");
            response
                .a11y_live(accesskit::Live::Polite, true)
                .a11y_automation_id("test.live");
        });
        let node = tree
            .by_automation_id("test.live")
            .expect("the live region exists");
        assert_eq!(node.live, Some(accesskit::Live::Polite));
        assert!(node.live_atomic);
    }

    #[test]
    fn an_alert_carries_its_text_as_the_accessible_name() {
        // An Alert is announced by its name, and AccessKit takes a Label's text
        // from `value`; an alert that only set `value` would be silent.
        let tree = render(|ui| {
            let response = ui.label("relay is not returning your game traffic");
            response
                .a11y_alert("relay is not returning your game traffic")
                .a11y_automation_id("test.alert");
        });
        let node = tree
            .by_automation_id("test.alert")
            .expect("the alert exists");
        assert_eq!(node.role, accesskit::Role::Alert);
        assert_eq!(
            node.label.as_deref(),
            Some("relay is not returning your game traffic")
        );
        assert!(node.has_name());
        assert_eq!(node.live, Some(accesskit::Live::Assertive));
        assert!(node.live_atomic);
    }

    #[test]
    fn a_combo_box_without_a_name_is_caught_by_the_invariant() {
        // Documents the gap this work closes: egui's ComboBox reports its
        // value but no name unless the caller names it.
        let tree = render(|ui| {
            ui.label("Relay");
            egui::ComboBox::from_id_salt("test_combo")
                .selected_text("Auto (fastest)")
                .show_ui(ui, |_| {});
        });
        let combo = tree
            .nodes
            .iter()
            .find(|node| node.role == accesskit::Role::ComboBox)
            .expect("the combo box is in the tree");
        assert!(
            !combo.has_name(),
            "a bare ComboBox has no name; the app must call accessible_name/labelled_by"
        );
        assert!(!tree.unnamed_inputs().is_empty());
    }

    #[test]
    fn automation_ids_are_unique_in_one_tree() {
        let tree = render(|ui| {
            for id in ["test.a", "test.b", "test.a"] {
                let response = ui.allocate_exact_size(vec2(40.0, 20.0), Sense::hover()).1;
                response.a11y_text("node").a11y_automation_id(id);
            }
        });
        assert_eq!(tree.duplicate_automation_ids(), vec!["test.a".to_string()]);
    }

    /// A compiled-in check that the collector really sees what eframe would
    /// hand to the Windows adapter: `WidgetInfo` for a standard widget, a
    /// custom node, and the automation id all coexist.
    #[test]
    fn widget_info_and_extra_node_properties_combine() {
        let tree = render(|ui| {
            let response = ui.allocate_exact_size(vec2(100.0, 30.0), Sense::click()).1;
            response.widget_info(|| {
                WidgetInfo::selected(accesskit::Role::CheckBox, true, true, "Share stats")
            });
            response
                .a11y_automation_id("test.checkbox")
                .a11y_live(accesskit::Live::Polite, false);
        });
        let node = tree
            .by_automation_id("test.checkbox")
            .expect("node properties survive WidgetInfo");
        assert_eq!(node.role, accesskit::Role::CheckBox);
        assert_eq!(node.label.as_deref(), Some("Share stats"));
        assert!(!node.disabled);
    }
}
