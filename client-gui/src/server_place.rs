//! Thin re-export shell for the server-location ladder.
//!
//! The ladder, the three offline indexes and the hosting-network classifier
//! moved into the shared core at WF-081 W2-3b S5a: they now live in
//! `lightspeed_client::app::location` behind the `server-location`/`geo-ip`
//! features (the GUI enables both). The frozen route surface
//! (`app/route_surface.rs`) still asks this module for the one pure formatting
//! helper it renders, so the shell keeps exactly that name; every other fact is
//! read from the core.

pub use lightspeed_client::app::location::geofeed_place_text;
