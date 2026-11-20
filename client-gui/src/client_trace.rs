//! Client-trace presentation: the human route caption.
//!
//! The scheduler, parser, refusal guard, bounded runner and the stable
//! `CLIENT_TRACE` log line moved into the shared core at WF-081 W2-3b S5b
//! (`lightspeed_client::app::trace`); both front ends now share that runtime.
//! What stays here is the localized caption the route card shows — hop count,
//! the nearest responding hop, and the provider/hosting name when the last hop's
//! prefix is in a shipped location index — which is presentation over the core's
//! `ClientRoute` fact.
//!
//! The trace is a local, display-only probe: it never feeds relay ranking and
//! nothing about it is sent to LightSpeed's relays or any third party (the
//! owner-approved Q13 disclosure is in `docs/privacy.md` /
//! `docs/data-dictionary.md`).

pub use lightspeed_client::app::dto::ClientRoute;
pub use lightspeed_client::app::trace::{info_line, now_unix_ms, GuiTrace};

use std::net::Ipv4Addr;

/// The provider/hosting network name for `address` from the shared location indexes.
///
/// Deliberately no city or region: a provider region from the cloud feed is a
/// fact, but a city for an arbitrary hop needs hop-geolocation data the product
/// does not license. An operator geofeed match names the operator - the same
/// display-only provenance the route card uses - and never a city for a hop.
#[allow(dead_code)] // presentation helper (only `caption` calls it, and the shell draws the trace row from route_surface)
fn network_label(address: Ipv4Addr) -> Option<String> {
    use lightspeed_client::app::location::{place, Place};
    match place(address) {
        Place::Cloud { provider, .. } => Some(provider.to_ascii_uppercase()),
        Place::Geofeed { provider, .. } => Some(provider.to_string()),
        Place::Hosting { name, .. } => Some(name.to_string()),
        Place::Country { .. } | Place::Unknown => None,
    }
}

/// The route-card caption for a finished trace, localized:
///
/// `Route to server: 12 hops · nearest responding hop 23 ms`
///
/// plus the provider/hosting address when the last responding hop's prefix is
/// in one of the shipped indexes. No city is ever appended — that needs hop
/// geolocation data, and this feature deliberately does not claim it.
#[allow(dead_code)] // see network_label: presentation-only; route_surface draws the trace row
pub fn caption(route: &ClientRoute) -> Option<String> {
    if route.hops == 0 {
        return None;
    }
    let hops = route.hops.to_string();
    let mut caption = crate::i18n::t_with("route.trace_route", &[("hops", hops.as_str())]);
    if let Some(rtt) = route.last_hop_rtt_ms {
        let rtt = rtt.to_string();
        caption.push_str(" \u{00b7} ");
        caption.push_str(&crate::i18n::t_with(
            "route.trace_nearest",
            &[("ms", rtt.as_str())],
        ));
    }
    if let Some(name) = route.last_hop.and_then(network_label) {
        caption.push_str(" \u{00b7} ");
        caption.push_str(&name);
    }
    Some(caption)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> Ipv4Addr {
        text.parse().expect("test address")
    }

    fn route_fixture() -> ClientRoute {
        ClientRoute {
            hops: 4,
            last_hop: Some(ip("203.0.113.9")),
            last_hop_rtt_ms: Some(23),
            complete: false,
        }
    }

    fn english<T>(body: impl FnOnce() -> T) -> T {
        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(Some("en"));
        let value = body();
        crate::i18n::set_language(None);
        value
    }

    #[test]
    fn the_caption_states_hops_and_the_nearest_reply() {
        let route = route_fixture();
        assert_eq!(
            english(|| caption(&route)).as_deref(),
            Some("Route to server: 4 hops \u{00b7} nearest responding hop 23 ms")
        );
    }

    #[test]
    fn a_cloud_hop_adds_the_provider_name_but_never_a_city() {
        // 3.237.221.191 is in the shipped AWS us-east-1 cloud range.
        let route = ClientRoute {
            hops: 9,
            last_hop: Some(ip("3.237.221.191")),
            last_hop_rtt_ms: Some(31),
            complete: false,
        };
        assert_eq!(
            english(|| caption(&route)).as_deref(),
            Some("Route to server: 9 hops \u{00b7} nearest responding hop 31 ms \u{00b7} AWS")
        );
    }

    #[test]
    fn a_hosting_hop_adds_the_network_name_but_never_a_city() {
        // The live G-Core Virginia server: the label is the operator, not JP -
        // and never the geofeed's city, which places the server, not a hop.
        let route = ClientRoute {
            hops: 12,
            last_hop: Some(ip("178.107.14.201")),
            last_hop_rtt_ms: Some(40),
            complete: false,
        };
        assert_eq!(
            english(|| caption(&route)).as_deref(),
            Some(
                "Route to server: 12 hops \u{00b7} nearest responding hop 40 ms \u{00b7} \
                 G-Core"
            )
        );
    }

    #[test]
    fn a_timeout_row_has_no_caption_without_any_hop() {
        let route = ClientRoute {
            hops: 0,
            last_hop: None,
            last_hop_rtt_ms: None,
            complete: false,
        };
        assert_eq!(english(|| caption(&route)), None);
    }
}
