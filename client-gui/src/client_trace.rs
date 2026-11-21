//! Client-trace presentation: the shared runtime and the fact types.
//!
//! The scheduler, parser, refusal guard, bounded runner and the stable
//! `CLIENT_TRACE` log line moved into the shared core at WF-081 W2-3b S5b
//! (`lightspeed_client::app::trace`); both front ends now share that runtime.
//!
//! The localized route caption is **not** built here: the route card renders
//! `route_surface::RouteTraceSurface` (hops + nearest reply + the provider
//! suffix for a last hop a shipped location index knows), so the GUI has exactly
//! one caption implementation. WF-081 W2-4a (task-53, R10) retired this module's
//! former `caption`/`network_label` helpers and their four presentation tests
//! when that suffix was wired into `route_surface`; the provider fact is the core
//! `app::location::place` answer, never duplicated here (guard G2 pins the
//! retirement).
//!
//! The trace is a local, display-only probe: it never feeds relay ranking and
//! nothing about it is sent to LightSpeed's relays or any third party (the
//! owner-approved Q13 disclosure is in `docs/privacy.md` /
//! `docs/data-dictionary.md`).

pub use lightspeed_client::app::dto::ClientRoute;
pub use lightspeed_client::app::trace::{info_line, now_unix_ms, GuiTrace};
