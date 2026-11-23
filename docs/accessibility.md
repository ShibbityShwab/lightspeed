# Accessibility

> Last updated: 2026-11-23

LightSpeed's desktop client (`lightspeed-gui`) is an immediate-mode egui/eframe
app: it paints its own widgets instead of using native Win32 controls. On
Windows it still exposes a standard **UI Automation (UIA)** tree, because egui
writes an [AccessKit](https://accesskit.dev/) tree and AccessKit's Windows
adapter (`accesskit_windows`) serves it to UIA. Windows Narrator, a third-party
screen reader, a magnifier, or an automation driver (PowerShell +
`UIAutomationClient`, WinAppDriver, Playwright-for-UIA) finds and operates the
window's controls through that tree.

The accessibility surface is treated as a **contract**, not a best-effort
extra: every interactive control has an accessible name and, where automation
needs it, a stable `AutomationId`; status changes that happen on their own are
live regions; and the same tree is rendered headlessly so CI and agent drivers
can assert it without a desktop session, a UIA client or a screen reader. The
gate is that tree plus the live UIA dump on a Windows host — not screen-reader
hardware testing (see [Limitations](#limitations)).

The code lives in `client-gui/src/app/a11y.rs`:

- `AccessKitResponse` — the one place a painted `egui::Response` is given a
  role, an accessible name, a stable automation id, an expansion state, a
  live-region priority or a screen-reader-only description.
- `automation` — the stable `AutomationId` strings (the contract below).
- `AxTree` — the frame's tree flattened into testable nodes; the
  `--a11y-dump` command and the accessibility tests both use it.

All helpers are no-ops when AccessKit is not active for the frame (no UIA client
and no screen reader), so calling them costs nothing during normal play.

---

## What the window exposes

- **Every interactive control reports a role, a name and its state.** egui
  supplies this for its own widgets (buttons, links, check boxes, combo boxes,
  text fields); custom-painted controls are given the same metadata through
  `AccessKitResponse`. Painted text is a `Label` (its text is the UIA value),
  the one bright action button is a `Button`, the painted map is an `Image`, a
  sheet is a `Dialog`, and the starvation/error lines are `Alert`s.
- **Every input-role node is named.** A combo box or text field with no name is
  unusable with a screen reader and unfindable by name; the tree helper
  `assert_every_input_is_named` is the invariant, and the current eleven dump
  screens contain zero unnamed input-role nodes.
- **Sheets are dialogs in the same tree.** Settings, Update, Game editor and
  Target picker are egui modals. AccessKit 0.37 keeps a modal's contents in the
  same tree as the window behind it, so an open sheet is a `Dialog` node whose
  title and controls are ordinary descendants (the `a11y`-module test
  `a_modal_is_part_of_the_same_tree` is the proof). The shell the sheet covers
  stays in the tree too.
- **The sheet ids sit on the title, not the dialog.** Each `*.sheet` id is on
  the localized title `Label` inside the `Dialog`; the `Dialog` node itself is
  unnamed. Target the id for the sheet, not the dialog node.
- **Disabled controls say so.** A control that is present but cannot act (for
  example the primary action before a relay is selected) reports disabled /
  `is_enabled=false`, and the shell ignores activation while it is disabled
  (`shell_action` checks `action.enabled` before applying). A disabled node may
  still advertise the `Click` action, so check enabled state before invoking.
- **Names are localized; ids are not.** The accessible name is the string the
  window shows (including its case and decoration), so it changes with the
  selected language. Automation must target the `AutomationId`, which never
  changes with the locale.
- **There is no accessibility mode or setting.** The tree is produced for any
  UIA client that asks (see [Lazy activation](#lazy-activation)); nothing has
  to be enabled first.

## Lazy activation

AccessKit's Windows adapter builds and maintains the tree only when a UIA client
asks for it: the window receives `WM_GETOBJECT` and `accesskit_windows`
initialises UIA lazily. Consequences worth knowing:

- A client that attaches immediately after launch can find the window before
  its tree exists — the adapter answers the first request with a placeholder and
  fills the tree only after the app has rendered a frame. Poll or retry until
  the expected nodes appear; `scripts/uia-tree.ps1` retries until at least 10
  descendants are visible (bounded by `-TimeoutSeconds`, default 20).
- Nothing in the app enables this. It is standard AccessKit behaviour, and it
  is why a UIA client is the trigger for the live tree.
- The offline `--a11y-dump` is not affected: it enables AccessKit on a plain
  egui context and renders frames with no window at all.

## Roles, names, states and actions

Roles follow the widget: `Button` for every painted button and console link,
`Link` for `ui.link` text links, `ComboBox` for the pickers, `TextInput` for
text fields, `CheckBox` for toggles, `Label` for painted text and data rows,
`Image` for the map, `Alert` for the starvation warning and the command-error
panel, `Dialog` for an open sheet.

Where the text lives:

| Node kind | AccessKit role | Text in |
| --- | --- | --- |
| State word, state detail, round-trip metric | `Label` | `value` |
| Data-rail rows, location row | `Label` | `value` (`LABEL: value` sentence) |
| Primary action, footer links, console links | `Button` | `name` (localized, uppercase where painted) |
| `Retry` links, `Clear target` | `Link` | `name` |
| Sheets' titles | `Label` | `value` (`/// TITLE` where the sheet paints the mark) |
| Combo boxes | `ComboBox` | `name` (label) + the selected value |
| Text fields | `TextInput` | `name` (label or hint) |
| Check boxes | `CheckBox` | `name` (+ the toggled state UIA exposes) |
| Painted map | `Image` | `name` = the localized description |
| Starvation warning, command-error panel | `Alert` | `name` = the whole sentence |
| Elevation / shell notice, `Game found` line | `Label` | `value` (live region) |

The state is what UIA already understands: enabled/disabled, focused, expanded
(the Details disclosure), live priority, and a check box's toggled state.
Actions are the ones the role supports: a `Button` or `Link` exposes `Click`
(UIA Invoke), a text field exposes its value and accepts `SetValue`, a combo box
exposes its selection.

The table above describes the AccessKit tree the dump records. Through UIA the
same nodes appear as control types: `Label` and `Alert` are `ControlType.Text`
(the text is the element's UIA `Name`, because AccessKit takes a label's name
from its value), `TextInput` is `ControlType.Edit`, `Link` is
`ControlType.Hyperlink`, `Button`/`ComboBox`/`CheckBox`/`Image` keep their
names, and a sheet's `Dialog` is a nested `ControlType.Window` with an empty
`Name`. A live assertion should target the automation id and the live
`control_type`, not the AccessKit role from the dump.

## Automation ids

The `automation` module in `client-gui/src/app/a11y.rs` holds the stable
`AutomationId` strings. AccessKit's Windows adapter exposes `Node::author_id` as
UIA's `AutomationId`, so these survive localization, layout and copy changes.
The namespace is dotted — `<screen>.<region>.<control>` — and two rules hold:

- **Renaming or removing an id is a breaking change for automation.** Treat the
  strings as public API: an automation driver, a test or a release check may
  depend on one.
- **Ids are unique within one tree.** Two nodes with one id make automation
  ambiguous; the accessibility tests assert per screen.

The tables below match the `automation` module exactly. The roles are the ones
the rendered tree reports.

### Main window — status

| AutomationId | Constant | Role | Control |
| --- | --- | --- | --- |
| `main.state.word` | `STATE_WORD` | Label | The state word (`NOT CONNECTED`, `OPTIMIZING`, `NO RETURN PATH`, ...) |
| `main.state.detail` | `STATE_DETAIL` | Label | The sub-line under the state word |
| `main.state.metric` | `ROUND_TRIP` | Label | The live relay-leg round trip (`42 MS RELAY LEG`) |

### Main window — route, notices and action

| AutomationId | Constant | Role | Control |
| --- | --- | --- | --- |
| `main.route.map` | `ROUTE_MAP` | Image | The painted telemetry map (described, not read) |
| `main.route.details` | `ROUTE_DETAILS` | Button | The `Details` disclosure, with its expanded state |
| `main.route.row.<label_key>` | `ROUTE_ROW` (prefix) | Label | One data-rail row; see below |
| `main.route.location` | `LOCATION_ROW` | Label | The location-ladder row |
| `main.warning.starvation` | `WARNING` | Alert | The starvation warning banner |
| `main.error.panel` | `ERROR_PANEL` | Alert | The command-error panel |
| `main.error.details` | `ERROR_DETAILS` | Button | The command-error panel's own `Details` disclosure |
| `main.notice` | `NOTICE` | Label | The elevation / shell notice line |
| `main.action.primary` | `ACTION_PRIMARY` | Button | The one primary action |
| `main.action.reason` | `ACTION_REASON` | Label | The disabled-reason line under the action |

`ROUTE_ROW` is the **prefix** `main.route.row.` — not a complete id. Each
data-rail row appends its catalogue `label_key`, so the suffix is the stable key,
never the localized label. Examples that exist on the seeded screens are
`main.route.row.route.relay`, `main.route.row.route.relayed`,
`main.route.row.route.game_server` and `main.route.row.route.relay_rtt`; the
`Details` rows use keys such as `route.server_leg`, `route.server_ping`,
`route.expected_path_label`, `route.server_leg_app`, `route.route_trace` and
`route.cannot_beat_direct`. A key is only in the tree while its row is shown, and
the node's text is the whole row sentence (`RELAY: 192.0.2.10:4434`).

### Main window — controls

| AutomationId | Constant | Role | Control |
| --- | --- | --- | --- |
| `main.relay.select` | `RELAY_SELECT` | ComboBox | The relay picker (`Auto (fastest)` or a pinned relay) |
| `main.relay.disconnect` | `RELAY_DISCONNECT` | Button | The `Disconnect` link shown while connected |
| `main.relay.retry` | `RELAY_RETRY` | Link | The `Retry` link shown when relay discovery failed or found nothing |
| `main.game.select` | `GAME_SELECT` | ComboBox | The game picker (auto, built-in, own profiles, `Manual / Custom…`) |
| `main.game.detected` | `GAME_DETECTED` | Label | The detected game's name on the `Game found:` line |
| `main.manual.server` | `MANUAL_SERVER` | TextInput | The manual server address field |
| `main.manual.port_range` | `MANUAL_PORT_RANGE` | TextInput | The custom UDP port-range field |
| `main.manual.save_name` | `MANUAL_SAVE_NAME` | TextInput | The `Save as` name field |
| `main.manual.save` | `MANUAL_SAVE` | Button | The `Save` button for the manual server |
| `main.manual.saved_servers` | `MANUAL_SAVED_SERVERS` | ComboBox | The saved-server selector |
| `main.target.process` | `TARGET_PROCESS` | Button | The `Target a process` front-door button |
| `main.target.clear` | `TARGET_CLEAR` | Link | The `Clear` link that drops the selected process |

### Main window — footer

| AutomationId | Constant | Role | Control |
| --- | --- | --- | --- |
| `main.footer.hide` | `FOOTER_HIDE` | Button | `Hide to tray`; present only when a real system tray exists (Windows) and `LIGHTSPEED_KEEP_VISIBLE` is unset |
| `main.footer.settings` | `FOOTER_SETTINGS` | Button | `Settings` |
| `main.footer.quit` | `FOOTER_QUIT` | Button | `Quit` |

### Settings sheet

| AutomationId | Constant | Role | Control |
| --- | --- | --- | --- |
| `settings.sheet` | `SETTINGS_SHEET` | Label | The sheet's title |
| `settings.button.close` | `SETTINGS_BUTTON_CLOSE` | Button | `Close` |
| `settings.language.select` | `SETTINGS_LANGUAGE` | ComboBox | The language picker |
| `settings.button.open_log` | `SETTINGS_LOG` | Button | `Open log file` |
| `settings.button.check_updates` | `SETTINGS_CHECK_UPDATES` | Button | `Check for updates` |
| `settings.checkbox.share_stats` | `SETTINGS_SHARE_STATS` | CheckBox | The anonymous-stats toggle |
| `settings.proxy.label` | `SETTINGS_PROXY_LABEL` | TextInput | The relay label field |
| `settings.proxy.addr` | `SETTINGS_PROXY_ADDR` | TextInput | The relay address field |
| `settings.proxy.add` | `SETTINGS_PROXY_ADD` | Button | `Add proxy` |
| `settings.proxy.reset` | `SETTINGS_PROXY_RESET` | Button | `Reset to defaults` |
| `settings.button.remeasure` | `SETTINGS_REMEASURE` | Button | `Re-measure relays` |
| `settings.button.refresh_relays` | `SETTINGS_REFRESH_RELAYS` | Button | `Refresh relays` |

### Update sheet

| AutomationId | Constant | Role | Control |
| --- | --- | --- | --- |
| `update.sheet` | `UPDATE_SHEET` | Label | The sheet's title |
| `update.button.close` | `UPDATE_BUTTON_CLOSE` | Button | `Close` |
| `update.button.install` | `UPDATE_BUTTON_INSTALL` | Button | `Install update` (only when a newer installer is available) |

### Game editor sheet

| AutomationId | Constant | Role | Control |
| --- | --- | --- | --- |
| `editor.sheet` | `EDITOR_SHEET` | Label | The sheet's title |
| `editor.button.close` | `EDITOR_BUTTON_CLOSE` | Button | `Close` |
| `editor.button.save` | `EDITOR_BUTTON_SAVE` | Button | `Save game` |
| `editor.button.delete` | `EDITOR_BUTTON_DELETE` | Button | `Delete` (only while editing an existing profile) |
| `editor.field.name` | `EDITOR_FIELD_NAME` | TextInput | The profile name |
| `editor.field.processes` | `EDITOR_FIELD_PROCESSES` | TextInput | The process names |
| `editor.field.udp_range` | `EDITOR_FIELD_UDP_RANGE` | TextInput | The UDP port range |
| `editor.field.notes` | `EDITOR_FIELD_NOTES` | TextInput | The notes |
| `editor.checkbox.dynamic_server` | `EDITOR_DYNAMIC_SERVER` | CheckBox | The `Server rotates between matches` toggle |

### Target picker sheet

| AutomationId | Constant | Role | Control |
| --- | --- | --- | --- |
| `picker.sheet` | `PICKER_SHEET` | Label | The sheet's title |
| `picker.button.close` | `PICKER_BUTTON_CLOSE` | Button | `Close` |
| `picker.search` | `PICKER_SEARCH` | TextInput | The process search field |
| `picker.button.refresh` | `PICKER_REFRESH` | Button | `Refresh` |

## Live regions

Screen readers do not read painted text that changes on its own, so the status
surfaces that change without user input are marked as AccessKit live regions.
AccessKit's Windows adapter raises `UIA_LiveRegionChanged` when a live node's UIA
name changes; a `Label`'s UIA name is its text, so the state word and the notices
announce their new text, and the two fault surfaces are `Alert`s carrying their
whole sentence as the name.

The adapter source also answers UIA's `LiveSetting` property, but a live probe of
this build could not read it — the UIA client reported `Unsupported Property` —
so neither the live script nor this document treats `LiveSetting` as observable.
The contract is the `live`/`live_atomic` pair on the accessibility tree (the
tests assert it) plus the raised event.

| Node | Setting | Announces when |
| --- | --- | --- |
| `main.state.word` | Polite, atomic | The state word changes: idle → connecting → optimizing, or → `NO RETURN PATH` / error |
| `main.game.detected` | Polite, atomic | The detected game appears or changes |
| `main.notice` | Polite, atomic | The elevation / shell notice appears or changes |
| `main.warning.starvation` | Assertive, atomic (`Alert`) | The relay stops returning the game server's traffic |
| `main.error.panel` | Assertive, atomic (`Alert`) | A command error panel appears or changes |

Only those five nodes are live regions; the metric, data rows and details are
ordinary `Label`s. "Atomic" means the region is read as a whole rather than as
only the changed words. Whether an announcement is spoken, and how, is the
screen reader's own setting: LightSpeed sets the UIA properties and raises the
event, it cannot force speech.

## The Details disclosure

The route card's `Details` row is a real interactive control, not a painted
label: `main.route.details` is a `Button` whose accessible name is the localized
`Details` label and whose **expanded** state mirrors the route card's open
flag. Activating it (pointer click, Space/Enter, or UIA
`ExpandCollapsePattern.Expand/Collapse`) toggles the
secondary data rows, which then appear as `main.route.row.<key>` nodes. The
error panel has its own `Details` disclosure inside `main.error.panel`,
exposed as `main.error.details` with the same expanded contract. A screen
reader's `ExpandCollapsePattern` now works: both disclosures advertise the
AccessKit Expand/Collapse actions and consume them (egui itself drops those
actions). UIA `InvokePattern` is not exposed on the row; pointer and Space/Enter
also toggle it.

## The painted map

The telemetry map is a canvas, so it cannot be "read" the way text can. It is
exposed as one `Image` node with the stable id `main.route.map` and a localized
accessible name (`a11y.route_map`) that says what the picture is and which relay
is active: *"World map of the relay fleet; active relay: ... Locations are
estimates, not measurements."* The marker dots and coordinates are painted only;
they are not in the tree.

That description is a summary, not a measurement: the authoritative values are
the state word, the round-trip metric and the data rows, which are text nodes
with their own ids. Do not parse the map's name for data, and do not treat it as
a second copy of the rail. This shell has no separate RTT-history canvas; every
number it shows is a text node.

## Keyboard and sheets

- **Tab / Shift+Tab** move focus among the focusable controls in egui's tree
  order; the app does not reorder focus traversal.
- **Space or Enter** on a focused click-sensing control performs its action.
  egui 0.37 maps keyboard focus + Space/Enter (and an AccessKit `Click` request)
  to `Response::clicked()`; the app's `activated` helper applies the same rule to
  its painted links, footer links, toggles and disclosures. A UIA Invoke request
  therefore lands on the same code path as a pointer click.
- **A visible focus ring** is painted by custom controls that call `focus_ring`:
  the 2 px `accent_text` ring is drawn outside the control rect (the design
  token `FOCUS_RING_W`), and focus does not change the fill. Stock egui widgets
  (text fields, combo boxes) keep egui's own focused visuals.
- **Text fields** take the usual editing keys. The manual server address is
  written to the config as it changes; the profile editor's fields are committed
  by its `Save game` control.
- **Sheets**: `Escape` (or a click on the dimmed backdrop) closes the topmost
  open sheet — egui's `ModalResponse::should_close` consumes Escape only for the
  top modal with no popup open — and every sheet also has a visible `Close`
  control with its own id. The design floor also asks Enter to confirm: there is
  no sheet-wide default button, so Enter activates whichever control has focus
  (the focused `Save`, `Install`, `Close`, ...). Closing a sheet by Escape or the
  backdrop discards the game editor's unsaved draft; there is no unsaved-draft
  guard.
- **Focus confinement**: the modal blocks pointer input behind it, but the
  accessibility gate does not assert that Tab stays inside an open sheet. The
  design-system audit lists that as unverified, and it is not claimed here.
- The painted data rows are data, not controls: they are read by a screen reader
  in tree order but are not focusable and are not in the tab order.

## Limitations

- **No screen-reader hardware run.** Narrator, NVDA, JAWS and third-party
  readers have not been driven against this build as part of the gate. The
  verification is the accessibility tree, the headless tests, the offline dump
  and a live UIA dump on a Windows host (see [How to verify](#how-to-verify)); a
  reader's speech output is not asserted.
- **The tray menu is OS-drawn.** The native menu created by the `tray-icon` crate
  is a Windows menu, not egui content, so its items are not in the window's UIA
  tree. Automation that needs tray actions must not assume an in-tree
  equivalent; the window's footer covers Settings, Hide and Quit. The
  hide-to-tray footer entry itself exists only when a real tray exists.
- **Isolate a live evidence run with `LIGHTSPEED_CONFIG_DIR` and
  `LIGHTSPEED_DATA_DIR`.** Setting `APPDATA`/`LOCALAPPDATA` alone does not
  isolate the Windows known-folder paths the app uses; point both overrides at a
  disposable directory, then delete it after the run. Use this for any
  `scripts/uia-tree.ps1` run that launches the app, so the owner's real
  `lightspeed.toml`, `relay-history.json` and trace log are not touched.- **The window must be visible.** When the client is hidden to the tray there is
  no on-screen window for a UIA client to enumerate. Show it first (tray click,
  or the single-instance behaviour on relaunch).
- **The tree activates lazily.** Until a UIA client asks, no tree is built (see
  [Lazy activation](#lazy-activation)). Poll for the first request.
- **The offline dump is fixed sample state.** It pins `language: "en"`, renders
  eleven screens from an offline app (no tray, no saved config, no network, no
  saved user data) and seeds RFC 5737 TEST-NET addresses (plus the loopback connect instruction). It is a schema of the
  UI, not a record of the machine, and it performs no host write or autostart
  probe: the offline app skips the platform reconcile the live app does.
- **The dump records a toggle's checked state (`toggled`), not text selection.**
  Each node carries role, name, value, description, automation id,
  live/live-atomic, expanded, toggled, disabled, focusable, clickable,
  `labelled_by` and children. A live UIA client can query richer patterns (text
  selection, scroll position); the dump covers what the tests and agent drivers
  need to find and assert a control.
- **The dump seeds almost every id.** Its eleven screens use 54 of the 56
  constants; the `main.route.row.` prefix appears as four distinct row ids, for
  56 distinct automation ids in total. The two deliberate exceptions are
  `main.footer.hide` (there is no tray offline; it is verified in the live tree)
  and `main.manual.saved_servers` (the saved-servers combo is hidden offline
  because it is local user data). The error panel, elevation notice, Manual
  fields, process-target picker/clear, editor Delete and update Install are all
  seeded by a screen.
- **No UIA client in CI.** CI runs the headless tree; the live end-to-end proof
  is `scripts/uia-tree.ps1` on a Windows host, recorded as evidence.
- **Live-region priority is not readable through UIA on this build.** A live
  probe got `Unsupported Property` for UIA's `LiveSetting`, and the live script
  therefore reports `live_setting: null` for every node. The event path is
  verified: a live UIA subscription observed `LiveRegionChanged` (event 20024)
  on `main.state.word` when the route changed to CONNECTED. The polite/assertive
  contract is asserted from the offline tree (`live`, `live_atomic`); the event
  is not observable in CI, and a screen reader's actual speech is unverified.
- **Inline popups are exposed, not "invisible".** A live UIA run opened the Game
  combo: the popup appears in the same tree as one `List` plus option `Button`s
  carrying each game name (and `Toggle` state for the relay picker), with the
  `New custom game…` link and scrollbars; closing removes them. The list itself
  is unnamed and the options have no stable ids, so automation should drive the
  closed combo by id and verify the popup's localized option text.
- **Screen-reader behaviour is platform-owned.** The app sets roles, names,
  properties and live-region events; what a reader says and when is its own
  setting.
- **Linux and macOS are headless-gated only.** The headless tree tests run on
  all three platforms in CI, but the live UIA path (AccessKit's Windows adapter)
  is Windows-only, and this project's live accessibility evidence is Windows.

## Adding a new control

1. **Reuse the helper.** For a stock egui widget, add
   `.accessible_name(...)` when it has no visible label (combo boxes, search
   fields) and `.a11y_automation_id(automation::YOUR_ID)`. For a custom-painted
   response, give it a role and name with
   `.a11y_widget(role, enabled, name)` / `.widget_info(|| WidgetInfo::labeled(...))`,
   or `.a11y_text(...)` for painted text.
2. **Add the id to the contract.** Declare the string in the `automation` module
   (`client-gui/src/app/a11y.rs`), dotted `<screen>.<region>.<control>`, unique
   in the tree, and add a row to the matching table in this document. Renaming an
   id later is a breaking change for automation.
3. **Say what changes.** If the text changes asynchronously (a state word, a
   notice, an alert), mark the node with `.a11y_live(Live::Polite, true)`, or use
   `.a11y_alert(text)` for an assertive `Alert` whose text is the accessible
   name. If it opens/closes a section, report `.a11y_disclosure(open)` so the
   expanded state and the AccessKit Expand/Collapse actions are advertised, and
   consume the action with `a11y::disclosure_action` (the two shipped
   disclosures show the pattern).
4. **Hide decoration.** Branding, ornamental rules and spinners that have an
   adjacent text label take `.a11y_hidden()`.
5. **Note the screen-reader-only description.** `a11y_description` exists for a
   hint that should not become a visible tooltip; it currently has no call site,
   so it is the one helper not exercised by the shipped tree.
6. **Prove it.** Add an assertion to `client-gui/src/app/tests.rs` (targeting the
   id, role and live/expanded state, not the localized label), then run the
   gate below.

## How to verify

The evidence path convention is `wat/evidence/<YYYY-MM-DD>/` (gitignored; for
example `wat/evidence/2026-11-23/uia/`). Name the dump, the transcript and the
commands used.

### The headless gate

```
cargo test -p lightspeed-gui
```

This renders real frames with AccessKit enabled and asserts the tree with no
window, no tray, no UIA client and no screen reader. The accessibility tests
cover:

- every screen has zero input-role nodes without a name and zero duplicate
  automation ids;
- `main.action.primary` is a `Button` with the state-driven localized name and
  exposes `Click`;
- `main.state.word` is a polite live region, `main.warning.starvation` is an
  `Alert` with assertive live state whose name is the warning sentence, and the
  connected screen has no warning node; the sub-line and the round-trip metric
  are separate named `Label` nodes (`main.state.detail`, `main.state.metric`);
- data-rail rows carry unique `main.route.row.<key>` ids and their text is the
  row sentence;
- `main.route.map` is an `Image` with a non-empty name; `main.route.details` is a
  `Button` whose expanded state follows the route card;
- each sheet screen has a `Dialog` plus its `*.sheet` title id and `*.button.close`
  id;
- a pinned non-English locale produces the translated accessible name;
- two dumps agree on the (screen, role, name, automation id) tuples;
- the dump sample carries only RFC 5737 TEST-NET addresses (plus the loopback connect instruction), no user profile path
  or host name, and no path except the documented `sample-game` fixture.

To inspect the whole tree by hand:

```
cargo test -p lightspeed-gui write_dump_for_inspection -- --ignored
```

That writes the full dump to `%TEMP%\v1125-dump.json`.

### The offline dump

`lightspeed-gui --a11y-dump <path>` renders the real window shell and every
sheet offline and writes the tree as JSON. It runs before the single-instance
guard and the tray, so it can run while a player's instance is open.

```
cargo build -p lightspeed-gui
.\target\debug\lightspeed-gui.exe --a11y-dump .\target\a11y-dump.json
```

- `--a11y-dump=<path>` is also accepted.
- Without a path it writes `lightspeed-a11y-dump.json` in the temp directory
  (`%TEMP%` on Windows) rather than failing.
- A GUI-subsystem process has no console, but stdout can be redirected: it prints
  the written path, so `--a11y-dump out.json > log` reports where the dump went.
  On failure it records the error to the crash log and exits with code 1.

The dump is schema 1: an object with `schema`, `app` (`lightspeed-gui`),
`version` (the build version), `language` (`en`), `window`
(`{"width": 320.0, "height": 760.0}`), `state` (a sentence saying this is offline
TEST-NET sample state) and `screens`. Each screen is `{"name": ..., "tree": ...}`;
each `tree` is `{"root": ..., "focus": ..., "nodes": [...]}` where `root`/`focus`
are opaque AccessKit node ids. Each node has:

| Key | Meaning |
| --- | --- |
| `id` | Opaque AccessKit node id (stable within a frame) |
| `role` | AccessKit role (`Button`, `Label`, `Image`, `Alert`, `Dialog`, ...) |
| `name` | UIA Name: the label, or `null` when the node is named by `labelled_by` |
| `value` | The text a `Label`-role node carries (`null` for named controls) |
| `description` | Accessible description (`null` in the shipped tree) |
| `automation_id` | UIA `AutomationId`, or `null` for nodes automation does not target |
| `live` | `null`, `"Polite"` or `"Assertive"` |
| `live_atomic` | Whether the whole region is announced |
| `expanded` | The disclosure's expanded state |
| `disabled` | UIA `IsEnabled` = false |
| `focusable` | The node supports the Focus action |
| `clickable` | The node supports the Click action |
| `labelled_by` | AccessKit node ids of the naming nodes |
| `children` | AccessKit node ids of the child nodes |

`children`/`labelled_by` hold node ids, so a reader can rebuild the hierarchy or
look nodes up by `automation_id`. Key order in the file is not meaningful.

The eleven screens are `main`, `main_connected`, `main_starved`,
`main_error`, `main_notice`, `main_manual`, `main_target`, `settings`,
`game_editor`, `target_picker` and `update`. The connected and starved screens
are seeded with RFC 5737 TEST-NET addresses so the route card and data rail render
real nodes; the sample holds no user profiles, no real machine paths and no
session history. The one path-shaped value is the synthetic target-picker
fixture `C:\Games\sample-game\sample-game.exe`, there only so the picker renders
a row.

### The live UIA tree

`scripts/uia-tree.ps1` is the live end-to-end half of the contract. It is
Windows PowerShell 5.1 only, uses only the built-in
`UIAutomationClient`/`UIAutomationTypes` assemblies, attaches to the running
window by pid or starts an exe, waits for the lazily-created UIA tree, walks it
and writes the live tree as JSON. It can also assert that expected nodes exist.

From a PowerShell session (a comma-separated id list binds as an array):

```
.\scripts\uia-tree.ps1 -ProcessId 1234 `
  -OutFile .\wat\evidence\2026-11-23\uia-tree.json `
  -ExpectAutomationId 'main.action.primary','main.state.word','main.footer.settings'
```

```
.\scripts\uia-tree.ps1 -ProcessId 1234 -TimeoutSeconds 20
```

Launched by the script (it warns that the single-instance guard exits 2 if
another instance is already running):

```
powershell -ExecutionPolicy Bypass -File .\scripts\uia-tree.ps1 `
  -ExePath .\target\debug\lightspeed-gui.exe `
  -OutFile .\wat\evidence\2026-11-23\uia-tree.json `
  -ExpectAutomationId main.action.primary
```

- `-ProcessId` attaches to an existing window; `-ExePath` starts one. A process
  the script started is stopped only when `-Stop` is given or the run failed; an
  attached instance is never touched.
- `-OutFile` defaults to `%TEMP%\lightspeed-uia-tree.json`.
- `-ExpectAutomationId` takes exact ids (case-sensitive) or wildcards
  (`main.route.row.*`, PowerShell `-like` matching); `-ExpectName` matches
  case-insensitively, exactly or as a substring. Missing expectations exit 1; a
  run that cannot get a tree at all exits 2 (no process, no window, or UIA never
  activated); success exits 0.
- Windows PowerShell's `-File` command line binds a comma-separated list to a
  single string; the script splits that form back into ids, so both
  `-ExpectAutomationId a,b` and one id per run work. From a PowerShell session,
  the array form above is clearer.
- `-TimeoutSeconds` (default 20) covers the window wait and the activation wait;
  the tree is polled until at least 10 descendants appear, and the run reports
  the attempts, the wait and the descendant count.
- stdout carries a human summary: window title, pid, node count, truncation,
  counts by control type, the automation ids found, missing expectations and the
  output path.

The live JSON is one object with `pid`, `exe`, `window_title`, `window_handle`,
`generated_at`, `node_count`, `scheme` (`raw`), `truncated`, an `activation`
block (`attempts`, `waited_ms`, `descendants`, `threshold`), the `expectations`
and their `missing_*` lists, and `tree`. `tree` is the nested root node; each
node has `name`, `automation_id`, `control_type` (UIA's programmatic name, for
example `ControlType.Button`), `localized_control_type`, `is_enabled`,
`is_offscreen`, `focusable`, `has_keyboard_focus`, `live_setting` (UIA's
`LiveSetting` where the node supports it — `null` on builds whose provider does
not answer that property, see [Live regions](#live-regions)), `bounds` (`x`,
`y`, `width`, `height`, or `null` for a layout-less element), `value` via the UIA
`ValuePattern` when the node supports it, `expand_collapse` (UIA
`ExpandCollapsePattern` state, `null` when unsupported), `toggle_state` (UIA
`TogglePattern`, e.g. `On` for the two checkboxes), `depth`, and `children`. Property reads
that hit a stale element during the walk fall back instead of failing the dump.

The live tree and the offline dump are complementary: the dump proves the
contract deterministically in the test gate, and the live run proves that the
same ids and names reach the real window through AccessKit's Windows adapter.
Neither asserts what a screen reader sounds like.

## Related

- `client-gui/src/app/a11y.rs` — the helper trait, the automation ids and the
  testable tree snapshot.
- `client-gui/src/app/a11y_dump.rs` — the offline dump and its schema.
- `client-gui/src/main.rs` — the `--a11y-dump` entry point.
- `client-gui/src/app.rs`, `client-gui/src/app/widgets.rs`,
  `client-gui/src/app/ledger.rs` — the shell and widget metadata.
- `scripts/uia-tree.ps1` — the live UIA tree dump and assertion script.
- [`docs/design/ui-audit.md`](design/ui-audit.md) — the visual accessibility
  floor (colour is never the only carrier of state).
- [`docs/design/design-system-2026.md`](design/design-system-2026.md) — focus
  ring, sheet and keyboard design rules and their verification status.
