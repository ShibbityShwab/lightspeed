use super::{
    lock_or_recover, plan_return_path_window, relay_reply_fault, relay_reply_fault_message,
    relay_reply_window_message, starvation_verdict, tray_state_for, warning_message_key,
    RelayReplyFault, ReplyFaultStreak, TrayState, WindowEvent, RELAY_REPLY_MIN_INTERCEPTED,
    RELAY_REPLY_STREAK, RELAY_REPLY_TIMEOUT,
};

#[test]
fn no_reply_watchdog_requires_time_traffic_and_a_return_path() {
    assert_eq!(
        relay_reply_fault(
            RELAY_REPLY_TIMEOUT - std::time::Duration::from_millis(1),
            100,
            0,
            0,
            None
        ),
        None
    );
    assert_eq!(
        relay_reply_fault(
            RELAY_REPLY_TIMEOUT,
            RELAY_REPLY_MIN_INTERCEPTED - 1,
            0,
            0,
            None
        ),
        None
    );
    // One reply in eight is the floor: above it the return path works and no
    // higher share is required. Below it a window may contribute to a fault,
    // but only through the streak.
    assert_eq!(
        relay_reply_fault(RELAY_REPLY_TIMEOUT, 100, 13, 13, None),
        None,
        "13 replies in 100 forwarded is above the one-in-eight floor"
    );
    assert_eq!(
        relay_reply_fault(RELAY_REPLY_TIMEOUT, 100, 25, 25, None),
        None
    );
    assert_eq!(
        relay_reply_fault(RELAY_REPLY_TIMEOUT, 100, 12, 12, None),
        Some(RelayReplyFault::TooFewReplies),
        "just under the floor is the only share that can feed a fault"
    );
}

#[test]
fn the_live_24_percent_window_is_healthy_on_every_reading() {
    // Run B, 2026-11-16: intercepted=54 injected=13 from_relay=13 - a 24.1%
    // return share on a relay measured 150 ms FASTER than the direct path.
    // The old quarter-share bar dropped the session to direct on this window.
    let window = relay_reply_fault(RELAY_REPLY_TIMEOUT, 54, 13, 13, Some((54, 13)));
    assert_eq!(
        window, None,
        "every reply that came back was injected; the return path works"
    );
    let mut streak = ReplyFaultStreak::default();
    assert_eq!(streak.observe(window), None);
}

#[test]
fn a_repeated_24_percent_share_never_condemns_the_relay() {
    // Requirement: the live 24.1% case must produce no fault on every window,
    // however many windows the warning logic reads.
    let mut streak = ReplyFaultStreak::default();
    for _ in 0..RELAY_REPLY_STREAK * 3 {
        let window = relay_reply_fault(RELAY_REPLY_TIMEOUT, 54, 13, 13, None);
        assert_eq!(
            streak.observe(window),
            None,
            "a low-but-working share must never become a fault"
        );
    }
}

#[test]
fn the_live_run_windows_replay_through_the_new_criterion() {
    // The unit-level harness for the live evidence in
    // `wat/evidence/2026-11-16/relay-hopping/`: every counter window from the
    // 2026-11-16 ROB RUN trace, passed through the pure decision. Run with
    // `--nocapture` to read it as a trace.
    struct Run {
        label: &'static str,
        intercepted: u64,
        injected: u64,
        from_relay: u64,
        windows: u32,
        expected: Option<RelayReplyFault>,
    }
    let runs = [
        Run {
            // Run A, 08:20:47: LAX carried 68 outbound for 91.199.82.166 and
            // returned nothing. Now needs the streak before it may fall back.
            label: "Run A LAX 08:20:47 (relay 207.246.106.36, server 91.199.82.166)",
            intercepted: 68,
            injected: 0,
            from_relay: 0,
            windows: RELAY_REPLY_STREAK,
            expected: Some(RelayReplyFault::NoReplyFromRelay),
        },
        Run {
            label: "Run A LAX retry 08:21:26 (attempt 1)",
            intercepted: 96,
            injected: 0,
            from_relay: 0,
            windows: RELAY_REPLY_STREAK,
            expected: Some(RelayReplyFault::NoReplyFromRelay),
        },
        Run {
            // Run B, 08:24:16: the damning case - a 150 ms-faster relay at a
            // 24.1% return share, dropped by the old quarter-share bar.
            label: "Run B EWR 08:24:16 (relay 207.246.124.117, server 3.237.212.91)",
            intercepted: 54,
            injected: 13,
            from_relay: 13,
            windows: RELAY_REPLY_STREAK * 3,
            expected: None,
        },
        Run {
            // Earlier run, 07:31:29: 73/12/12 = 16.4%, also called starved by
            // the old bar (12*4 = 48 < 73).
            label: "Earlier run LAX 07:31:29 (73 intercepted, 12 injected, 12 returned)",
            intercepted: 73,
            injected: 12,
            from_relay: 12,
            windows: RELAY_REPLY_STREAK * 3,
            expected: None,
        },
    ];
    for run in runs {
        let mut streak = ReplyFaultStreak::default();
        let mut verdict = None;
        for _ in 0..run.windows {
            let window = relay_reply_fault(
                RELAY_REPLY_TIMEOUT,
                run.intercepted,
                run.injected,
                run.from_relay,
                None,
            );
            verdict = streak.observe(window);
        }
        println!(
            "{}: intercepted={} injected={} from_relay={} -> {:?}",
            run.label, run.intercepted, run.injected, run.from_relay, verdict
        );
        assert_eq!(verdict, run.expected, "{}", run.label);
    }
}

#[test]
fn a_broken_injector_is_still_detected_after_the_streak() {
    // from_relay >= 5 with injected == 0 is the local path, not the relay.
    let window = relay_reply_fault(RELAY_REPLY_TIMEOUT, 600, 0, 40, None);
    assert_eq!(window, Some(RelayReplyFault::InjectionFailed));
    let mut streak = ReplyFaultStreak::default();
    assert_eq!(streak.observe(window), None);
    assert_eq!(
        streak.observe(window),
        Some(RelayReplyFault::InjectionFailed),
        "a sustained injection fault still raises the warning"
    );
    let server = addr("198.51.100.23:36703");
    let plan = plan_return_path_window(
        None,
        Some(RelayReplyFault::InjectionFailed),
        window,
        server,
        None,
    );
    let warning = plan.warning.expect("the warning is raised");
    assert_eq!(warning.fault, RelayReplyFault::InjectionFailed);
    assert_eq!(
        warning_message_key(warning.fault),
        "route.injection_failed_warning",
        "a local injection fault must not blame the relay in the banner"
    );
    assert!(!plan.switch_to_direct);
}

#[test]
fn different_faults_do_not_share_a_streak() {
    let mut streak = ReplyFaultStreak::default();
    let no_reply = relay_reply_fault(RELAY_REPLY_TIMEOUT, 60, 0, 0, None);
    let injector = relay_reply_fault(RELAY_REPLY_TIMEOUT, 600, 0, 40, None);
    assert_eq!(streak.observe(no_reply), None);
    assert_eq!(
        streak.observe(injector),
        None,
        "a different fault restarts the count; the absence must be sustained"
    );
    assert_eq!(
        streak.observe(injector),
        Some(RelayReplyFault::InjectionFailed)
    );
}

#[test]
fn only_a_confirmed_no_reply_window_is_a_starvation_verdict() {
    assert!(
        starvation_verdict(RelayReplyFault::NoReplyFromRelay, None),
        "the first confirmed NoReplyFromRelay window is the finding to remember"
    );
    assert!(
        !starvation_verdict(
            RelayReplyFault::NoReplyFromRelay,
            Some(RelayReplyFault::NoReplyFromRelay)
        ),
        "the rest of the episode repeats a finding already published"
    );
    assert!(
        starvation_verdict(
            RelayReplyFault::NoReplyFromRelay,
            Some(RelayReplyFault::InjectionFailed)
        ),
        "a NoReply finding after a different confirmed fault is new evidence about the relay"
    );
    assert!(
        !starvation_verdict(RelayReplyFault::TooFewReplies, None),
        "a low return share is normal game UDP, not a starvation finding"
    );
    assert!(
        !starvation_verdict(RelayReplyFault::InjectionFailed, None),
        "a local injection fault says nothing about the relay"
    );
}

#[test]
fn the_settings_policy_describes_the_warn_only_behaviour() {
    // The old hover copy promised re-tests "after 20 s, then 45 s, then 90 s"
    // while the code did 30 s/120 s/park, and the automatic fallback has since
    // been removed entirely. The replacement must describe what the client
    // really does and must leave the decision to the user.
    let _guard = crate::i18n::test_lock();
    crate::i18n::set_language(Some("en"));
    let copy = crate::i18n::t("settings.starvation_policy").to_lowercase();
    for stale in ["20 s", "45 s", "90 s", "re-test", "retry"] {
        assert!(
            !copy.contains(stale),
            "the retired re-test promise is still in the settings copy: {stale} in {copy}"
        );
    }
    assert!(copy.contains("warn"), "the copy must say it warns: {copy}");
    assert!(
        copy.contains("keeps relaying"),
        "the copy must say the client keeps relaying: {copy}"
    );
    assert!(
        copy.contains("stop optimizing"),
        "the copy must name the manual route: {copy}"
    );
    assert!(
        copy.contains("pick another relay"),
        "the copy must name the alternative: {copy}"
    );
    // The route-card banner is the §2.7 B measurement, not a verdict, and names
    // the remedy's cost (M16).
    let banner = crate::i18n::t("route.relay_not_returning").to_lowercase();
    assert!(banner.contains("no game traffic"), "{banner}");
    assert!(banner.contains("stop optimizing"), "{banner}");
    assert!(banner.contains("pick another relay"), "{banner}");
    assert!(
        banner.contains("restarts"),
        "the banner must name the cost of the remedy: {banner}"
    );
    let injection = crate::i18n::t("route.injection_failed_warning").to_lowercase();
    assert!(
        injection.contains("log") && injection.contains("maintenance"),
        "a local injection fault points at the in-app log path: {injection}"
    );
    assert!(
        !injection.contains("relay is not returning"),
        "an injection failure must not blame the relay: {injection}"
    );
    let state = crate::i18n::t("state.not_returning");
    assert_ne!(state.as_ref(), "state.not_returning");
    assert!(!state.trim().is_empty());
    crate::i18n::set_language(None);
}

#[test]
fn the_w2_copy_deck_rows_are_installed() {
    // §2.7 B is a W2-2 deliverable, not a suggestion: the four warning/route
    // rows, the two Settings policy rows and the bare location rung values are
    // pinned here so a catalogue edit cannot quietly regress to the pre-W2 copy.
    let _guard = crate::i18n::test_lock();
    crate::i18n::set_language(Some("en"));
    assert!(crate::i18n::t("route.relay_not_returning").contains("No game traffic is coming back"));
    assert!(
        crate::i18n::t("route.injection_failed_warning").contains("Settings \u{2192} Maintenance")
    );
    let expected = crate::i18n::t("route.expected_path");
    assert!(
        !expected.contains("(L + S)"),
        "the placeholder-leaking suffix is retired: {expected}"
    );
    assert!(expected.contains("(relay leg + server leg)"), "{expected}");
    assert_eq!(
        crate::i18n::t("route.expected_path_label").as_ref(),
        "Expected path"
    );
    assert_eq!(crate::i18n::t("route.route_trace").as_ref(), "Path trace");

    // The hold number is generated from HOLD_FALLBACK (120 s = 2 minutes); the
    // retired row rendered LOCK_SETTLE (3 s), the spec's 40x-wrong number (C5).
    assert_eq!(super::HOLD_FALLBACK.as_secs() / 60, 2);
    let minutes = (super::HOLD_FALLBACK.as_secs() / 60).to_string();
    let hold = crate::i18n::t_with("settings.hold_policy", &[("minutes", minutes.as_str())]);
    assert!(hold.contains("2 minutes"), "{hold}");
    assert!(
        !hold.contains("3s") && !hold.contains("direct path"),
        "{hold}"
    );

    // The `Location` label owns the words "Location"; the rung value must be
    // the bare provenance phrase, never a second "Server location: ..." (M9/m4).
    let label = crate::i18n::t("route.location_label");
    for key in [
        "route.location_estimate",
        "route.location_provider",
        "route.location_hosting",
        "route.location_unknown",
    ] {
        let value = crate::i18n::t(key);
        assert!(
            !value.starts_with("Server location:"),
            "{key} repeats the Location label: {value}"
        );
        assert!(
            value.as_ref() != label.as_ref(),
            "{key} duplicates its own label"
        );
        assert!(!value.trim().is_empty(), "{key} is empty");
    }
    assert!(crate::i18n::t("route.location_geofeed").contains("{place}"));
    // The Settings section heading stopped being a hardcoded English literal.
    assert_eq!(crate::i18n::t("settings.routing").as_ref(), "Routing");
    crate::i18n::set_language(None);
}

#[test]
fn every_locale_carries_the_warn_only_strings() {
    // All nine catalogs must carry the new strings (the parity test in `i18n`
    // enforces the key set; this proves the values are real text). ja/ko/zh-Hans
    // keep English because the bundled font subset cannot draw those glyphs.
    let _guard = crate::i18n::test_lock();
    let keys = [
        "settings.starvation_policy",
        "route.relay_not_returning",
        "route.injection_failed_warning",
        "state.not_returning",
    ];
    // The W2-2b §2.7 B rows are the spec's source strings in every catalog until
    // the Q5 translation pipeline runs (translation debt, recorded).
    let may_fallback = [
        "settings.starvation_policy",
        "route.relay_not_returning",
        "route.injection_failed_warning",
    ];
    // Read the English values once, before the per-locale loop: resolving them
    // inside the loop would pin the global language back to English and make
    // every later comparison compare English to English.
    crate::i18n::set_language(Some("en"));
    let english: Vec<String> = keys
        .iter()
        .map(|key| crate::i18n::t(key).into_owned())
        .collect();
    for locale in crate::i18n::LOCALES {
        crate::i18n::set_language(Some(locale.tag));
        for (index, key) in keys.iter().enumerate() {
            let text = crate::i18n::t(key);
            assert_ne!(
                text.as_ref(),
                *key,
                "{} is missing the warn-only string {key}",
                locale.tag
            );
            assert!(!text.trim().is_empty(), "{} has an empty {key}", locale.tag);
            if matches!(locale.tag, "ja" | "ko" | "zh-Hans") {
                assert_eq!(
                    text.as_ref(),
                    english[index],
                    "{} must keep the English string for {key} (font subset)",
                    locale.tag
                );
            } else if locale.tag != "en" && !may_fallback.contains(key) {
                // Only the W2-2b deck rows may currently be English in a
                // translated catalog (Q5 translation debt, recorded in
                // `wat/evidence/2026-11-20/w2-2/locale-fallback.md`).
                assert_ne!(
                    text.as_ref(),
                    english[index],
                    "{} must translate {key}",
                    locale.tag
                );
            }
        }
    }
    crate::i18n::set_language(None);
}

#[test]
fn a_healthy_server_share_outranks_a_starving_window() {
    // The live case: the relay-wide window read 152 intercepted against 16
    // returned, while this server's own counters were 528 sent and 520
    // replied - 98.5%. Warning about it would have accused a working relay.
    assert_eq!(
        relay_reply_fault(RELAY_REPLY_TIMEOUT, 152, 16, 16, Some((528, 520))),
        None
    );
    // A thin flow cannot vouch for the relay: it has not measured enough.
    assert_eq!(
        relay_reply_fault(RELAY_REPLY_TIMEOUT, 152, 16, 16, Some((5, 5))),
        Some(RelayReplyFault::TooFewReplies)
    );
    // A server whose own counters are poor is still a fault.
    assert_eq!(
        relay_reply_fault(RELAY_REPLY_TIMEOUT, 152, 16, 16, Some((400, 20))),
        Some(RelayReplyFault::TooFewReplies)
    );
}

#[test]
fn a_silent_relay_and_a_broken_injector_are_told_apart() {
    // The user-visible question this answers: three relays all "starving"
    // is the relay fleet, while a relay that answered and produced nothing
    // on screen is ours. The counters distinguish them, so the log must too.
    assert_eq!(
        relay_reply_fault(RELAY_REPLY_TIMEOUT, 100, 0, 0, None),
        Some(RelayReplyFault::NoReplyFromRelay)
    );
    assert_eq!(
        relay_reply_fault(RELAY_REPLY_TIMEOUT, 100, 0, 9, None),
        Some(RelayReplyFault::InjectionFailed)
    );
    let message = relay_reply_fault_message(
        RelayReplyFault::InjectionFailed,
        100,
        0,
        9,
        57,
        Some("WinDivert inject error: invalid handle"),
        0,
        None,
    );
    assert!(message.contains("local injection path"), "{message}");
    assert!(message.contains("from_relay=9"), "{message}");
    assert!(message.contains("errors=57"), "{message}");
    assert!(message.contains("invalid handle"), "{message}");
    let silent = relay_reply_fault_message(
        RelayReplyFault::NoReplyFromRelay,
        100,
        0,
        0,
        0,
        None,
        0,
        None,
    );
    assert!(silent.contains("from_relay=0"), "{silent}");
    assert!(!silent.contains("local injection"), "{silent}");
    assert!(silent.contains("errors=0"), "{silent}");
    assert!(silent.contains("last_error=none"), "{silent}");
    // The warn-only replacement for "switching direct": the log must say the
    // relay is kept and the player warned, because the automatic path switch was
    // removed on 2026-11-20.
    for message in [&message, &silent] {
        assert!(
            message.contains("no automatic path switch"),
            "the trace must state the warn-only contract: {message}"
        );
        assert!(
            !message.to_lowercase().contains("switching direct"),
            "the removed automatic switch must not survive in the log: {message}"
        );
    }
}

#[test]
fn a_local_send_failure_is_named_instead_of_blaming_the_relay() {
    // The reported #168 failure: every send was rejected with WSAEMSGSIZE,
    // the relay never saw a datagram, and the trace called it relay silence.
    let message = relay_reply_fault_message(
        RelayReplyFault::NoReplyFromRelay,
        120,
        0,
        0,
        0,
        None,
        7,
        Some("A message sent on a datagram socket was larger than the internal message buffer (os error 10040)"),
    );
    assert!(
        message.contains("The relay cannot be the cause"),
        "{message}"
    );
    assert!(message.contains("10040"), "{message}");
    assert!(message.contains("local path-MTU"), "{message}");
    assert!(
        !message.contains("Relay return path is starving"),
        "a local send failure must not be reported as relay starvation: {message}"
    );
    assert!(
        message.contains("no automatic path switch"),
        "the send failure is still a warning, never an automatic switch: {message}"
    );
}

#[test]
fn every_relay_reply_fault_carries_the_error_fields() {
    // A confirmed fault is the moment a user asks "which side failed", so every
    // arm - not just the injection-failure one - must carry the error
    // counters and the last fatal message. The sibling
    // `a_silent_relay_and_a_broken_injector_are_told_apart` separates the
    // three causes; this keeps each arm's trace line honest, including the
    // WinDivert invalid-handle case that killed the data plane in issue
    // #167.
    let last_error = Some("WinDivert receive handle is invalid (os error 6); interception stopped");
    for (fault, intercepted, injected, from_relay) in [
        (RelayReplyFault::NoReplyFromRelay, 68, 0, 0),
        (RelayReplyFault::InjectionFailed, 600, 0, 40),
        (RelayReplyFault::TooFewReplies, 152, 16, 16),
    ] {
        let message = relay_reply_fault_message(
            fault,
            intercepted,
            injected,
            from_relay,
            3,
            last_error,
            0,
            None,
        );
        assert!(
            message.contains("errors=3"),
            "{fault:?} dropped the error count: {message}"
        );
        assert!(
            message.contains("os error 6"),
            "{fault:?} dropped the last error: {message}"
        );
        assert!(
            !message.contains("last_error=none"),
            "a recorded error must not print as none: {message}"
        );
    }
}

#[test]
fn a_judged_window_is_traced_even_when_it_is_healthy() {
    // QA finding F1: a judged-but-healthy window left no trace, so a live
    // "no false positive" proof was absence-based. Every judgement now emits
    // the counters the decision read, the share they produce and the streak.
    let mut streak = ReplyFaultStreak::default();
    let healthy = relay_reply_fault(RELAY_REPLY_TIMEOUT, 54, 13, 13, None);
    assert_eq!(streak.observe(healthy), None);
    let line = relay_reply_window_message(
        54,
        13,
        13,
        (7, 41),
        shadow_window(3, 0, 7, 1, 1),
        &streak,
        healthy,
    );
    for field in [
        "intercepted=54",
        "injected=13",
        "from_relay=13",
        "share=24.1%",
        "streak=0",
        "verdict=healthy",
        // The engine samplers' counters ride on this INFO line because
        // their own accepted/rejected traces are debug-level and the
        // shipped file sink is INFO; without them the falsifiers are
        // unmeasurable in release.
        "probe_accepted=7",
        "probe_rejected=41",
        "shadow_accepted=3",
        // The rejection reasons are the diagnosis: ICMP-blocked
        // (`no_icmp`), impossible-fast pairing (`floor`), lost reply
        // (`timeout`), changed path (`relay`). Task-21 had 204 rejections
        // with no way to tell which.
        "shadow_rejected=9(no_icmp=0,floor=7,timeout=1,relay=1)",
    ] {
        assert!(line.contains(field), "missing {field} in {line}");
    }
    // A window with no shadow rejections still carries the (empty)
    // breakdown, so an operator can tell "no rejections" from "field absent".
    let no_rejections = relay_reply_window_message(
        12,
        1,
        9,
        (0, 0),
        shadow_window(0, 0, 0, 0, 0),
        &streak,
        None,
    );
    assert!(
        no_rejections.contains("shadow_rejected=0(no_icmp=0,floor=0,timeout=0,relay=0)"),
        "{no_rejections}"
    );
    // A faulting window names its verdict and the streak it advances; the
    // second one is the window the warning fires on.
    let mut streak = ReplyFaultStreak::default();
    let fault = relay_reply_fault(RELAY_REPLY_TIMEOUT, 60, 0, 0, None);
    assert_eq!(streak.observe(fault), None);
    let first = relay_reply_window_message(
        60,
        0,
        0,
        (7, 42),
        shadow_window(3, 0, 7, 1, 1),
        &streak,
        fault,
    );
    assert!(first.contains("streak=1"), "{first}");
    assert!(first.contains("verdict=no_reply_from_relay"), "{first}");
    assert!(first.contains("share=0.0%"), "{first}");
    assert!(streak.observe(fault).is_some());
    let second = relay_reply_window_message(
        60,
        0,
        0,
        (7, 42),
        shadow_window(3, 0, 7, 1, 1),
        &streak,
        fault,
    );
    assert!(second.contains("streak=2"), "{second}");
    // Every window verdict has a stable name in the trace.
    for (window, verdict) in [
        (
            Some(RelayReplyFault::TooFewReplies),
            "verdict=too_few_replies",
        ),
        (
            Some(RelayReplyFault::InjectionFailed),
            "verdict=injection_failed",
        ),
        (None, "verdict=healthy"),
    ] {
        let line = relay_reply_window_message(
            12,
            1,
            9,
            (0, 0),
            shadow_window(0, 0, 0, 0, 0),
            &streak,
            window,
        );
        assert!(line.contains(verdict), "missing {verdict} in {line}");
    }
}

#[test]
fn a_poisoned_mutex_is_recovered_not_panicked() {
    let mutex = std::sync::Mutex::new(String::from("state"));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = mutex.lock().unwrap();
        panic!("simulated panic while holding the mutex");
    }));
    assert!(result.is_err(), "the panic must poison the mutex");
    assert_eq!(*lock_or_recover(&mutex), "state");
}

#[test]
fn client_leg_samples_pair_onto_data_addresses_not_health_ports() {
    // The regression that kept a live match on the wrong relay. A measurement
    // is taken against `ip:HEALTH_PORT`, and the entry's address is the
    // advertised data port, so comparing whole addresses matches nothing and
    // the chooser silently returns `None` forever. Pairing is by IP.
    let entries = vec![
        ProxyEntry::custom(addr("207.246.106.36:4434"), "LAX"),
        ProxyEntry::custom(addr("207.246.124.117:4434"), "EWR"),
    ];
    let measured = vec![
        (addr("207.246.106.36:8080"), Some(sample(244, 88))),
        (addr("207.246.124.117:8080"), Some(sample(286, 31))),
    ];

    let legs = pair_relay_legs(&entries, &measured);

    assert_eq!(legs.len(), 2, "both relays must survive the pairing");
    assert_eq!(
        legs[0].0,
        addr("207.246.106.36:4434"),
        "the stored key must be the data address, not the health port"
    );
    assert_eq!(legs[0].1, Some(sample(244, 88)));
    assert_eq!(legs[1].0, addr("207.246.124.117:4434"));
    assert_eq!(legs[1].1, Some(sample(286, 31)));
}

#[test]
fn pairing_keeps_entry_order_and_omits_relays_with_no_sample() {
    // Entry order matters downstream (ties break on the relay address), and a
    // relay that did not answer must be absent rather than reported as zero.
    let entries = vec![
        ProxyEntry::custom(addr("10.0.0.1:4434"), "a"),
        ProxyEntry::custom(addr("10.0.0.2:4434"), "b"),
        ProxyEntry::custom(addr("10.0.0.3:4434"), "c"),
    ];
    let measured = vec![
        (addr("10.0.0.3:8080"), Some(sample(90, 5))),
        (addr("10.0.0.1:8080"), Some(sample(10, 2))),
    ];

    let legs = pair_relay_legs(&entries, &measured);

    assert_eq!(
        legs.iter().map(|(a, _)| *a).collect::<Vec<_>>(),
        vec![addr("10.0.0.1:4434"), addr("10.0.0.3:4434")],
        "entry order is honoured and the relay with no sample is omitted"
    );
}

#[test]
fn a_failed_probe_pairs_as_none_rather_than_a_zero_latency() {
    // A relay that answered `None` has no usable leg: it must not look like a
    // zero-millisecond relay, which would win every comparison.
    let entries = vec![ProxyEntry::custom(addr("10.0.0.1:4434"), "a")];
    let measured = vec![(addr("10.0.0.1:8080"), None)];

    let legs = pair_relay_legs(&entries, &measured);

    assert_eq!(legs.len(), 1);
    assert_eq!(legs[0].1, None, "a failure stays a failure");
}

#[test]
fn a_health_port_winner_resolves_to_its_data_plane_relay() {
    // The race measures `IP:8080`, the proxy list stores `IP:4434`. A
    // full-address lookup matched nothing, so auto-select never connected.
    let entries = vec![
        ProxyEntry::discovered("relay-a", addr("203.0.113.10:4434"), "A"),
        ProxyEntry::discovered("relay-b", addr("203.0.113.11:4434"), "B"),
    ];
    assert_eq!(
        proxy_index_for(&entries, addr("203.0.113.11:8080")),
        Some(1)
    );
    assert_eq!(proxy_index_for(&entries, addr("203.0.113.12:8080")), None);
}

#[test]
fn the_relay_in_use_is_recognised_by_ip_across_ports() {
    assert!(same_relay_endpoint(
        addr("203.0.113.10:4434"),
        addr("203.0.113.10:8080")
    ));
    assert!(!same_relay_endpoint(
        addr("203.0.113.10:4434"),
        addr("203.0.113.11:8080")
    ));
}
use super::{
    apply_discovery_result, apply_target_choice, close_decision, compare_to_direct,
    direct_claim_label, direct_us_for, draft_reasons, early_triangle_floor, fresh_direct_us,
    game_editor_body, game_pick_rows, games, monitor_local_top, pair_relay_legs, picker_status,
    ping_for_server, process_picker, proxy_index_for, rank_without_destination,
    ranking_state_label, refreshed_server_ping, relay_cannot_beat_direct, same_relay_endpoint,
    save_user_profiles, scored_for_relay, server_from_fields, server_leg_state, should_apply_race,
    should_race, target_for_pick, target_front_door, target_summary, validate_draft,
    window_height_limit, CloseDecision, EditorActions, GameEditorDraft, GamePickRow,
    MeasuredServerLeg, PickerStatus, ProcessListState, ProcessPicker, ProcessTarget,
    ServerLegState, ServerPing, ShadowWindowCounters, TargetRowAction, MAX_WINDOW_HEIGHT,
    MIN_WINDOW_HEIGHT, PRIOR_UNCERTAINTY_US, SCREEN_BOTTOM_RESERVE, SERVER_PING_FRESH_SECS,
    TITLE_BAR_ALLOWANCE,
};
use crate::config::ProxyEntry;
use crate::discovery::{DiscoveryOutcome, RelayInfo};
use crate::processes::ProcessEntry;
use std::net::SocketAddrV4;
use std::time::Duration;

fn addr(s: &str) -> SocketAddrV4 {
    s.parse().expect("test address")
}

/// A measured relay leg: median and spread, both in milliseconds.
fn sample(median_ms: u64, jitter_ms: u64) -> crate::discovery::RelaySample {
    crate::discovery::RelaySample {
        median: Duration::from_millis(median_ms),
        jitter: Duration::from_millis(jitter_ms),
    }
}

/// Renders one frame with the app's fonts and theme installed.
///
/// Catches the typography wiring headlessly: an unregistered or misspelled
/// named family panics inside epaint the moment text uses it, which is
/// exactly how the bundled Inter crash reached a real user.
#[test]
fn the_theme_renders_a_frame_using_the_emphasis_family() {
    let ctx = eframe::egui::Context::default();
    super::install_fonts(&ctx, None);
    super::apply_theme(&ctx);

    let raw = eframe::egui::RawInput {
        screen_rect: Some(eframe::egui::Rect::from_min_size(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::vec2(420.0, 800.0),
        )),
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        ui.label(eframe::egui::RichText::new("Emphasis").family(super::semibold()));
        ui.label("body");
        ui.monospace("207.246.106.36:4434");
    });
    out.textures_delta.clear();
    assert!(!out.shapes.is_empty(), "a frame with text drew nothing");
}

/// The localized render, proven end to end rather than by proxy: the same
/// widget code must draw different text under two languages, and the
/// German frame must contain the German string and not the English one.
#[test]
fn a_localized_frame_draws_the_translated_string() {
    let _guard = crate::i18n::test_lock();
    let drawn = |key: &str| {
        let ctx = eframe::egui::Context::default();
        super::install_fonts(&ctx, Some("de"));
        super::apply_theme(&ctx);
        let raw = eframe::egui::RawInput {
            screen_rect: Some(eframe::egui::Rect::from_min_size(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::vec2(420.0, 800.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            super::section_label(ui, &crate::i18n::t(key));
        });
        out.textures_delta.clear();
        format!("{:?}", out.shapes)
    };

    crate::i18n::set_language(Some("de"));
    let german = format!("{:?}", drawn("settings.privacy"));
    crate::i18n::set_language(None);
    let english = format!("{:?}", drawn("settings.privacy"));

    assert!(
        german.contains("Datenschutz"),
        "the German frame did not draw the German string: {german}"
    );
    assert!(
        english.contains("Privacy"),
        "the English frame did not draw the English string: {english}"
    );
    assert_ne!(german, english, "both languages drew identical frames");
}

/// Every character in every catalog must be drawable by the families the app
/// installs.
///
/// Asserted against the real `FontDefinitions` the app builds and the face
/// bytes inside them, not a copy of the chain, so dropping a face from a
/// family or adding a catalog character nothing carries both fail here. (The
/// bundled Inter is a 230-codepoint Latin subset, and `semibold` once chained
/// only Inter faces - the bug this gate was written for. It carries no
/// exemption now that Q19 closed the picker gaps.)
#[test]
fn every_catalog_glyph_is_drawable_in_every_family() {
    use ab_glyph::{Font as _, FontArc};

    let catalogs: [(&str, &str); 9] = [
        ("de", include_str!("../../locales/de.toml")),
        ("en", include_str!("../../locales/en.toml")),
        ("es", include_str!("../../locales/es.toml")),
        ("fr", include_str!("../../locales/fr.toml")),
        ("ja", include_str!("../../locales/ja.toml")),
        ("ko", include_str!("../../locales/ko.toml")),
        ("pt-BR", include_str!("../../locales/pt-BR.toml")),
        ("ru", include_str!("../../locales/ru.toml")),
        ("zh-Hans", include_str!("../../locales/zh-Hans.toml")),
    ];

    let defs = super::font_definitions(Some("zh-Hans"));
    let faces: Vec<(String, FontArc)> = defs
        .font_data
        .iter()
        .map(|(name, data)| {
            let bytes = data.font.as_ref().as_ref();
            let face = FontArc::try_from_vec(bytes.to_vec()).expect("a bundled face must parse");
            (name.clone(), face)
        })
        .collect();

    let mut undrawable: Vec<String> = Vec::new();
    for (family, chain) in &defs.families {
        for (tag, catalog) in catalogs {
            for ch in catalog.chars().filter(|ch| !ch.is_ascii()) {
                let drawable = chain.iter().any(|name| {
                    faces
                        .iter()
                        .find(|(face_name, _)| face_name == name)
                        .is_some_and(|(_, face)| face.glyph_id(ch).0 != 0)
                });
                if !drawable {
                    undrawable.push(format!("{family:?} / {tag}: U+{:04X} {ch}", ch as u32));
                }
            }
        }
    }
    // The language picker prints each locale's own name; those strings are not
    // in a catalog, so a face that cannot draw `简体中文` would still fail in the
    // Settings sheet. Assert them with the same chains. (`·`, `→`, `…` are the
    // only non-ASCII marks the shell types; every other mark is painted.)
    //
    // Q19 closed the picker gap: `국`, `简` and `体` are now in the bundled CJK
    // subsets (regenerated by `scripts/fonts/subset_gui_fonts.py`), so this
    // gate has no exemption left to carry.
    for (family, chain) in &defs.families {
        for locale in crate::i18n::LOCALES {
            for ch in locale.native_name.chars().filter(|ch| !ch.is_ascii()) {
                let drawable = chain.iter().any(|name| {
                    faces
                        .iter()
                        .find(|(face_name, _)| face_name == name)
                        .is_some_and(|(_, face)| face.glyph_id(ch).0 != 0)
                });
                if !drawable {
                    undrawable.push(format!(
                        "{family:?} / native_name {}: U+{:04X} {ch}",
                        locale.tag, ch as u32
                    ));
                }
            }
        }
    }

    undrawable.sort();
    undrawable.dedup();
    assert!(
        undrawable.is_empty(),
        "catalog characters no installed face can draw: {undrawable:?}"
    );
}

/// The language picker's own names must not just have a cmap entry - they must
/// draw ink.
///
/// `every_catalog_glyph_is_drawable_in_every_family` proves the picker
/// characters resolve to a bundled face; Q19's defect was that `국`, `简`, `体`
/// resolved to nothing bundled at all, and a glyph id alone would not catch a
/// face that maps a codepoint to a blank or `.notdef` outline. This rasterises
/// every non-ASCII picker character through the real installed chains and
/// requires non-empty coverage: no tofu, per family, per locale. Regenerated
/// assets come from `scripts/fonts/subset_gui_fonts.py`; reverting them to the
/// pre-Q19 cuts fails here with the codepoint named.
#[test]
fn the_language_picker_names_draw_ink_in_every_family() {
    use ab_glyph::{Font as _, FontArc};

    let defs = super::font_definitions(Some("zh-Hans"));
    let faces: Vec<(String, FontArc)> = defs
        .font_data
        .iter()
        .map(|(name, data)| {
            let bytes = data.font.as_ref().as_ref();
            let face = FontArc::try_from_vec(bytes.to_vec()).expect("a bundled face must parse");
            (name.clone(), face)
        })
        .collect();

    let mut failures: Vec<String> = Vec::new();
    for (family, chain) in &defs.families {
        for locale in crate::i18n::LOCALES {
            for ch in locale.native_name.chars().filter(|ch| !ch.is_ascii()) {
                let resolved = chain.iter().find_map(|name| {
                    faces
                        .iter()
                        .find(|(face_name, face)| face_name == name && face.glyph_id(ch).0 != 0)
                        .map(|(face_name, face)| (face_name, face))
                });
                let Some((face_name, face)) = resolved else {
                    failures.push(format!(
                        "{family:?} / {}: U+{:04X} {ch} resolves to no bundled face",
                        locale.tag, ch as u32
                    ));
                    continue;
                };
                let glyph = face.glyph_id(ch).with_scale(24.0);
                let Some(outlined) = face.outline_glyph(glyph) else {
                    failures.push(format!(
                        "{family:?} / {}: U+{:04X} {ch} resolves to {face_name} but has no outline",
                        locale.tag, ch as u32
                    ));
                    continue;
                };
                let mut ink = 0usize;
                outlined.draw(|_x, _y, coverage| {
                    if coverage > 0.0 {
                        ink += 1;
                    }
                });
                if ink == 0 {
                    failures.push(format!(
                        "{family:?} / {}: U+{:04X} {ch} resolves to {face_name} but rasterises to blank",
                        locale.tag, ch as u32
                    ));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "the language picker would draw tofu: {failures:?}"
    );

    // Calibration: the check must be able to see blank, or the assertion above
    // proves nothing. U+00A0 is in every subset's safety set and maps to a space
    // glyph with no outline - `ab_glyph` reports it as `None`, so a `Some`
    // outline with non-zero ink above is a real glyph, not a .notdef box.
    let blank = '\u{00A0}';
    let (_, face) = faces
        .iter()
        .find(|(_, face)| face.glyph_id(blank).0 != 0)
        .expect("the safety set carries U+00A0 in a bundled face");
    assert!(
        face.outline_glyph(face.glyph_id(blank).with_scale(24.0))
            .is_none(),
        "the outline API must report a blank glyph as empty, or the ink check is meaningless"
    );
}

#[test]
fn window_close_hides_when_a_real_tray_exists() {
    assert_eq!(close_decision(true, false, true), CloseDecision::Hide);
}

#[test]
fn window_close_exits_when_there_is_no_tray_to_hide_to() {
    assert_eq!(close_decision(false, false, true), CloseDecision::Exit);
}

#[test]
fn tray_quit_exits_even_with_a_tray_and_an_open_window() {
    assert_eq!(close_decision(true, true, false), CloseDecision::Exit);
    assert_eq!(close_decision(true, true, true), CloseDecision::Exit);
    assert_eq!(close_decision(false, true, true), CloseDecision::Exit);
}

#[test]
fn no_close_and_no_quit_keeps_running() {
    assert_eq!(close_decision(true, false, false), CloseDecision::Ignore);
    assert_eq!(close_decision(false, false, false), CloseDecision::Ignore);
}

#[test]
fn game_table_matches_the_client_registry() {
    let entries = games();
    let registry = lightspeed_client::games::all_game_keys();
    // The client registry owns the entry count; this test only proves the
    // GUI table is derived from it without drift.
    assert_eq!(entries.len(), registry.len());
    for (entry, (key, display)) in entries.iter().zip(registry) {
        assert_eq!(entry.key, key);
        assert_eq!(entry.display, display);
        assert!(
            entry.default_port > 0,
            "{} resolved to no default port",
            entry.key
        );
    }
}

#[test]
fn the_window_never_auto_sizes_past_the_bottom_of_the_screen() {
    // 2560x1440 at 125%: 1152 pt tall. The Manual form needed more than
    // that, and the old fixed 1400 pt cap put the action bar under the
    // taskbar. The window must now end above the taskbar allowance.
    let limit = window_height_limit(Some(1152.0), Some(31.0));
    assert_eq!(limit, 1152.0 - SCREEN_BOTTOM_RESERVE - 31.0);
    assert!(31.0 + limit <= 1152.0 - 48.0, "clears a 48 pt taskbar");

    // A window placed lower on the screen gets less room.
    assert!(window_height_limit(Some(1152.0), Some(300.0)) < limit);

    // Position unknown (Wayland), or a window straddling two monitors with
    // its top on the other one: assume a title bar at the top of this one.
    let assumed = 1152.0 - SCREEN_BOTTOM_RESERVE - TITLE_BAR_ALLOWANCE;
    assert_eq!(window_height_limit(Some(1152.0), None), assumed);
    assert_eq!(window_height_limit(Some(1152.0), Some(-900.0)), assumed);
    assert_eq!(window_height_limit(Some(1152.0), Some(1500.0)), assumed);
}

#[test]
fn a_window_on_a_stacked_mixed_dpi_monitor_is_measured_on_that_monitor() {
    // A 1920x1080 primary at 100% on top, a 2560x1440 secondary at 125%
    // below it: the secondary's origin is desktop y = 1080 px. The window's
    // client area starts 50 px down the secondary.
    let ppp = 1.25;
    let monitor_height = 1440.0 / ppp; // 1152 pt
    let top = monitor_local_top(1080 + 50, 1080, ppp);
    assert_eq!(top, 40.0);
    let limit = window_height_limit(Some(monitor_height), Some(top));
    assert_eq!(limit, 1152.0 - SCREEN_BOTTOM_RESERVE - 40.0);

    // Scaling the desktop coordinate first folds the primary's 1080 px in
    // at the secondary's scale: 904 pt "down" a 1152 pt monitor, which would
    // have pinned the window to its minimum for no reason.
    let desktop_scaled = (1080.0 + 50.0) / ppp;
    assert_eq!(
        window_height_limit(Some(monitor_height), Some(desktop_scaled)),
        MIN_WINDOW_HEIGHT
    );

    // The secondary above the primary instead (a negative origin): the
    // same window 50 px down it measures the same.
    assert_eq!(monitor_local_top(-1440 + 50, -1440, ppp), 40.0);

    // Side by side with the secondary's top 300 px lower than the
    // primary's: measured from the secondary's own top, not desktop 0.
    assert_eq!(monitor_local_top(300 + 50, 300, ppp), 40.0);
}

#[test]
fn the_window_height_limit_keeps_the_fixed_bounds() {
    // A tall monitor still stops at the fixed maximum.
    assert_eq!(
        window_height_limit(Some(2160.0), Some(31.0)),
        MAX_WINDOW_HEIGHT
    );
    // No monitor reading: the fixed maximum, as before.
    assert_eq!(window_height_limit(None, Some(31.0)), MAX_WINDOW_HEIGHT);
    assert_eq!(window_height_limit(Some(0.0), None), MAX_WINDOW_HEIGHT);
    // A tiny screen or a window parked near the bottom never shrinks the
    // window below its minimum; the scroll region absorbs the rest.
    assert_eq!(
        window_height_limit(Some(480.0), Some(31.0)),
        MIN_WINDOW_HEIGHT
    );
    assert_eq!(
        window_height_limit(Some(1152.0), Some(1000.0)),
        MIN_WINDOW_HEIGHT
    );
}

#[test]
fn auto_select_races_only_when_disconnected_with_relays() {
    assert!(should_race(true, false, 2));
    assert!(!should_race(true, true, 2), "already connected");
    assert!(!should_race(true, false, 0), "no relays to race");
    assert!(!should_race(false, false, 2), "user pinned a relay");
}

#[test]
fn a_pinned_relay_a_live_connection_or_a_known_server_beats_a_late_race_result() {
    assert!(should_apply_race(true, false, false));
    assert!(
        !should_apply_race(false, false, false),
        "user pinned a relay"
    );
    assert!(!should_apply_race(true, true, false), "connected meanwhile");
    assert!(
        !should_apply_race(true, false, true),
        "a known game server governs the relay choice"
    );
}

// â”€â”€ The server leg on the route surface â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// A scored relay as the ranking composes it: `total_leg_us` is the client
/// leg plus the destination leg plus the prior's uncertainty charge, exactly
/// as `relay_choice::score_with` builds it.
fn scored_relay(
    ip: [u8; 4],
    client_leg_us: u64,
    dest_leg_us: u64,
    source: lightspeed_client::DestinationLegSource,
    jitter_us: u64,
) -> lightspeed_client::ScoredRelay {
    use lightspeed_client::DestinationLegSource;
    let penalty = match source {
        DestinationLegSource::Measured | DestinationLegSource::GeometricFloor => 0,
        DestinationLegSource::RegionPrior | DestinationLegSource::Unknown => PRIOR_UNCERTAINTY_US,
    };
    lightspeed_client::ScoredRelay {
        relay: SocketAddrV4::new(std::net::Ipv4Addr::from(ip), 4434),
        total_leg_us: client_leg_us + dest_leg_us + penalty,
        dest_leg_us,
        dest_leg_prior_us: dest_leg_us,
        client_leg_us,
        dest_source: source,
        jitter_us,
    }
}

/// A direct-ping record for `ip`, in milliseconds, `age_secs` old.
fn ping_for(ip: [u8; 4], ms: f32, age_secs: u64) -> ServerPing {
    ServerPing {
        server: std::net::Ipv4Addr::from(ip),
        ms,
        age_secs,
    }
}

/// The shadow sampler's counters as one judged window reads them; `rejected`
/// is the sum of the four reasons, exactly as `ShadowCounters::rejected()`
/// defines it.
fn shadow_window(
    accepted: u64,
    no_icmp: u64,
    behind: u64,
    timeout: u64,
    relay: u64,
) -> ShadowWindowCounters {
    ShadowWindowCounters {
        accepted,
        rejected: no_icmp + behind + timeout + relay,
        no_icmp_floor: no_icmp,
        behind_floor: behind,
        timeout,
        stale_relay: relay,
    }
}

// â”€â”€ The no-position fallback (live 2026-11-19 replay) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// The candidate rows the live 2026-11-19 session ranked, with its measured
/// client legs (one-RTT `/health` probes) and the relay names the GUI builds.
///
/// The server was a G-Core address (`178.107.14.0/24`) that R1's hosting index
/// reports as "hosting network - location unknown", so `chooser_point()` is
/// `None` and the destination-aware ranking has no exact server position.
fn live_gcore_candidates() -> Vec<lightspeed_client::RelayCandidate> {
    let row = |ip: &str, node: &str, client_us: u64| lightspeed_client::RelayCandidate {
        relay: ip.parse().expect("relay address"),
        region: Some(crate::discovery::friendly_label(node)),
        node_id: Some(node.to_string()),
        client_leg_us: Some(client_us),
        client_jitter_us: 0,
    };
    vec![
        row("207.246.106.36:4434", "relay-lax-1", 216_500),
        row("207.246.124.117:4434", "relay-ewr-1", 304_700),
        row("45.77.32.236:4434", "relay-sgp-1", 45_500),
        row("95.179.164.138:4434", "relay-fra", 200_200),
        row("167.179.77.127:4434", "relay-nrt", 120_900),
        row("65.20.92.201:4434", "relay-bom-1", 95_500),
        row("65.20.99.61:4434", "relay-mad-1", 196_100),
        row("45.76.114.93:4434", "relay-syd-1", 138_400),
    ]
}

#[test]
fn the_live_g_core_server_with_no_position_falls_back_from_sgp_to_lax() {
    use std::net::Ipv4Addr;
    // The live ROB RUN server. R1's ladder finds it on a hosting network and says
    // "location unknown", so there is no point to rank on.
    let server = Ipv4Addr::new(178, 107, 14, 59);
    let candidates = live_gcore_candidates();
    // The session's measured direct ICMP ping.
    let direct_us = Some(315_000);

    // With no record, the measured client leg decides (all destination priors are
    // unknown): SGP is 45.5 ms from the client. This is the defect being replayed,
    // and it is also the required fallback order - never the relay address.
    let before = rank_without_destination(
        server,
        &candidates,
        direct_us,
        &crate::relay_history::RelayHistory::default(),
    );
    assert_eq!(before.proven, 0, "nothing is proven without records");
    assert_eq!(
        before.ranked.len(),
        candidates.len(),
        "an unproven relay is still a candidate"
    );
    let client_legs: Vec<u64> = before.ranked.iter().map(|row| row.client_leg_us).collect();
    let mut sorted = client_legs.clone();
    sorted.sort_unstable();
    assert_eq!(
        client_legs, sorted,
        "with no records the fallback orders by measured client leg"
    );
    assert_eq!(
        before.ranked[0].relay.ip(),
        &Ipv4Addr::new(45, 77, 32, 236),
        "the live defect: relay-sgp-1 won on the shortest client leg"
    );

    // The live session's carry records: SGP starved this server (enough forwards,
    // no reply at all) and the two North-America relays are proven to have
    // carried it, while the rest are unknown. A proven record is the one piece of
    // real evidence a position-less choice has.
    let mut history = crate::relay_history::RelayHistory::default();
    history.record_starvation(addr("45.77.32.236:4434"), server);
    history.record(addr("207.246.106.36:4434"), server, 500, 400);
    history.record(addr("207.246.124.117:4434"), server, 500, 400);

    let after = rank_without_destination(server, &candidates, direct_us, &history);
    assert_eq!(
        after.ranked[0].relay.ip(),
        &Ipv4Addr::new(207, 246, 106, 36),
        "with SGP demoted and LAX/EWR proven, the ranking must pick LAX - not SGP \
         on the client leg and not anything on the relay address"
    );
    assert_eq!(
        after.ranked[0].client_leg_us, 216_500,
        "LAX wins the proven pair on the measured client leg"
    );
    assert_eq!(
        after.proven, 2,
        "LAX and EWR are the relays proven to carry this server"
    );
    assert_eq!(
        after.skipped_starved, 1,
        "the starved SGP record is excluded"
    );
}

#[test]
fn a_starved_carry_record_demotes_a_relay_without_the_watchdogs_failed_flag() {
    use std::net::Ipv4Addr;
    let server = Ipv4Addr::new(178, 107, 14, 59);
    let candidates = live_gcore_candidates();
    let mut history = crate::relay_history::RelayHistory::default();
    // The signature case: the relay carried traffic and nothing came back, but no
    // exhausted-chain verdict was written, so `is_starved` (which reads `failed`)
    // stays false. The no-position fallback still must not re-select it.
    history.record(addr("45.77.32.236:4434"), server, 4_522, 0);
    assert!(
        !history.is_starved(addr("45.77.32.236:4434"), server),
        "the watchdog-verdict semantics are unchanged"
    );
    let ranking = rank_without_destination(server, &candidates, Some(315_000), &history);
    assert_eq!(ranking.skipped_starved, 1);
    assert_eq!(ranking.proven, 0, "nobody proved anything in this variant");
    assert_ne!(
        ranking.ranked[0].relay.ip(),
        &Ipv4Addr::new(45, 77, 32, 236),
        "a starved record must not be re-selected just because `failed` is false"
    );
}

#[test]
fn the_no_position_fallback_ignores_the_untrusted_region_prior() {
    use std::net::Ipv4Addr;
    let server = Ipv4Addr::new(178, 107, 14, 59);
    let candidates = live_gcore_candidates();
    // The relay's registration ack resolves this hosting range through the free
    // DB-IP data to JP, so the engine's region prior believes the server is in
    // APAC. With no exact server position that prior must not order the relays:
    // it is the guess that picked relay-sgp-1 in the live session. The fallback
    // is the measured client legs.
    let estimator = lightspeed_client::ensure_destination_estimator();
    estimator.set_destination_region(server, "apac");
    let mut history = crate::relay_history::RelayHistory::default();
    history.record_starvation(addr("45.77.32.236:4434"), server);
    let ranking = rank_without_destination(server, &candidates, Some(315_000), &history);
    // If the APAC prior ranked, it would floor SGP/NRT/BOM/SYD at the 315 ms
    // direct ping and then put NRT (the nearest APAC relay in that map) first.
    // The fallback ignores it and takes the shortest measured client leg after
    // the starved SGP - BOM at 95.5 ms.
    assert_eq!(
        ranking.ranked[0].relay.ip(),
        &Ipv4Addr::new(65, 20, 92, 201),
        "the untrusted region prior must not rank when the server position is unknown"
    );
    assert_eq!(
        ranking.ranked[0].client_leg_us, 95_500,
        "the order is the measured client leg, not the region prior"
    );
    assert_eq!(
        ranking.ranked[0].dest_source,
        lightspeed_client::DestinationLegSource::Unknown,
        "no destination leg was used as evidence"
    );
}

// The ranking-order tests that used to live here were moved to the engine
// suite when the interim re-rank was removed (task-17): the order is
// `client/src/route/relay_choice.rs`'s, so its regression tests live beside
// it - `the_whole_path_total_outranks_the_destination_bucket`,
// `a_measured_destination_leg_is_compared_on_the_whole_path` and
// `ties_are_deterministic_and_jitter_breaks_an_exact_total_tie`, plus the
// client-leg tie-break
// `a_floor_tie_with_equal_priors_is_broken_by_the_client_leg_not_the_address`.

#[test]
fn the_server_leg_state_says_how_the_leg_was_obtained() {
    use lightspeed_client::DestinationLegSource;
    let measured = scored_relay(
        [10, 0, 0, 1],
        100_000,
        50_000,
        DestinationLegSource::Measured,
        0,
    );
    let prior = scored_relay(
        [10, 0, 0, 2],
        100_000,
        40_000,
        DestinationLegSource::RegionPrior,
        0,
    );
    let floored = scored_relay(
        [10, 0, 0, 3],
        220_170,
        63_300,
        DestinationLegSource::GeometricFloor,
        0,
    );
    let unknown = scored_relay(
        [10, 0, 0, 4],
        100_000,
        40_000,
        DestinationLegSource::Unknown,
        0,
    );
    let published = MeasuredServerLeg {
        dest_leg_us: 51_000,
        samples: 12,
        age: Some(Duration::from_secs(3)),
    };

    assert_eq!(
        server_leg_state(Some(&measured), Some(published), None),
        ServerLegState::Measured(published)
    );
    assert_eq!(
        server_leg_state(Some(&prior), None, None),
        ServerLegState::Prior { prior_us: 40_000 },
        "an estimate the ranking did not measure is stated as the prior it is"
    );
    assert_eq!(
        server_leg_state(Some(&floored), None, None),
        ServerLegState::Bounded { floor_us: 63_300 },
        "a triangle bound is a proof of a lower bound, not a measurement"
    );
    assert_eq!(
        server_leg_state(Some(&unknown), None, None),
        ServerLegState::Unknown
    );
    assert_eq!(server_leg_state(None, None, None), ServerLegState::Unknown);
    // The ranking row says measured but no published estimate survives: say
    // nothing the estimator cannot back up.
    assert_eq!(
        server_leg_state(Some(&measured), None, None),
        ServerLegState::Unknown
    );
}

#[test]
fn a_fresh_direct_ping_bounds_the_prior_before_the_next_ranking() {
    // The live task-21 shape: the initial choice was RegionPrior(~41 ms) and
    // the ping landed seconds later (direct 279 ms - 219 ms client leg = a
    // 60 ms floor), but the ranking only re-runs every 600 s, so the bound
    // was not shown for 9 m 58 s. The surface now states the same bound as
    // soon as the ping is fresh: no re-rank, no relay change, no measured
    // claim.
    use lightspeed_client::DestinationLegSource;
    let prior = scored_relay(
        [10, 0, 0, 2],
        219_000,
        41_000,
        DestinationLegSource::RegionPrior,
        0,
    );
    assert_eq!(
        server_leg_state(Some(&prior), None, Some(60_000)),
        ServerLegState::Bounded { floor_us: 60_000 },
        "a provable floor above the prior is shown as the bound, early"
    );
    // A floor at or below the prior proves nothing the prior does not claim.
    assert_eq!(
        server_leg_state(Some(&prior), None, Some(30_000)),
        ServerLegState::Prior { prior_us: 41_000 }
    );
    assert_eq!(
        server_leg_state(Some(&prior), None, Some(41_000)),
        ServerLegState::Prior { prior_us: 41_000 },
        "an equal floor is not a stronger statement"
    );
    // No fresh ping => the prior state, unchanged.
    assert_eq!(
        server_leg_state(Some(&prior), None, None),
        ServerLegState::Prior { prior_us: 41_000 }
    );
    // A candidate with no prior at all still gets the provable bound.
    let unknown = scored_relay([10, 0, 0, 4], 219_000, 0, DestinationLegSource::Unknown, 0);
    assert_eq!(
        server_leg_state(Some(&unknown), None, Some(60_000)),
        ServerLegState::Bounded { floor_us: 60_000 }
    );
    // A published measurement always outranks a proof.
    let published = MeasuredServerLeg {
        dest_leg_us: 51_000,
        samples: 12,
        age: Some(Duration::from_secs(3)),
    };
    assert_eq!(
        server_leg_state(Some(&prior), Some(published), Some(60_000)),
        ServerLegState::Measured(published)
    );
}

#[test]
fn the_early_bound_needs_a_fresh_ping_and_a_measured_client_leg() {
    let server = std::net::Ipv4Addr::new(91, 199, 82, 162);
    let ping = ping_for([91, 199, 82, 162], 279.474, 12);
    // 279.474 ms direct - 219.170 ms client leg = 60.304 ms floor: the same
    // arithmetic `relay_choice::apply_floor` performs at the next ranking.
    assert_eq!(
        early_triangle_floor(Some(ping), server, Some(219_170)),
        Some(60_304)
    );
    // A stale ping must not manufacture a bound (one freshness rule, shared
    // with the ranking's floor and the header metric).
    assert_eq!(
        early_triangle_floor(
            Some(ping_for(
                [91, 199, 82, 162],
                279.474,
                SERVER_PING_FRESH_SECS + 1
            )),
            server,
            Some(219_170)
        ),
        None
    );
    assert_eq!(early_triangle_floor(None, server, Some(219_170)), None);
    // No measured client leg: nothing to subtract, so no proof.
    assert_eq!(early_triangle_floor(Some(ping), server, None), None);
    assert_eq!(
        early_triangle_floor(
            Some(ping_for([91, 199, 82, 162], 0.0, 1)),
            server,
            Some(219_170)
        ),
        None
    );
}

#[test]
fn the_triangle_verdict_needs_two_fresh_client_measurements() {
    let server = std::net::Ipv4Addr::new(3, 237, 142, 3);
    let fresh = ping_for([3, 237, 142, 3], 283.474, 12);
    // EWR with one-RTT legs: a 305.84 ms client leg against a 283.47 ms
    // direct ping. A relay hop cannot make that path shorter, whatever the
    // server leg is.
    assert_eq!(
        relay_cannot_beat_direct(Some(305_840), Some(fresh), server),
        Some((305_840, 283_474))
    );
    // LAX's 220.17 ms leg does not reach the direct ping: no verdict.
    assert_eq!(
        relay_cannot_beat_direct(Some(220_170), Some(fresh), server),
        None
    );
    assert_eq!(relay_cannot_beat_direct(None, Some(fresh), server), None);
    assert_eq!(relay_cannot_beat_direct(Some(305_840), None, server), None);
    // Staleness is the same rule the ranking's floor and the header use.
    assert_eq!(
        relay_cannot_beat_direct(
            Some(305_840),
            Some(ping_for(
                [3, 237, 142, 3],
                283.474,
                SERVER_PING_FRESH_SECS + 1
            )),
            server
        ),
        None
    );
}

// â”€â”€ The direct ping is keyed to the server it measured (task-25) â”€â”€â”€â”€â”€
//
// The live defect: `follow_game_server` runs before `poll_relay_health`
// refreshes `server_ping`, so a choice for a NEW game server could consume the
// PREVIOUS server's fresh ping as its triangle floor (observed: a choice for
// `91.199.82.221` carried `direct 281` from `91.199.82.162`). Every consumer
// must ask for the ping of the server it is describing.

#[test]
fn a_ping_measured_for_another_server_is_not_evidence_for_this_one() {
    let server_a = std::net::Ipv4Addr::new(91, 199, 82, 162);
    let server_b = std::net::Ipv4Addr::new(91, 199, 82, 221);
    let ping_a = ping_for([91, 199, 82, 162], 279.474, 12);

    // The ranking floor and the choice-trace comparison both read this value.
    assert_eq!(direct_us_for(Some(ping_a), server_b), None);
    let claim = direct_claim_label(direct_us_for(Some(ping_a), server_b), 286_000);
    assert!(
        claim.contains("direct ping not measured"),
        "a foreign ping must not produce a comparison: {claim}"
    );
    // The triangle verdict and the early Server-leg bound refuse it too.
    assert_eq!(
        relay_cannot_beat_direct(Some(305_840), Some(ping_a), server_b),
        None
    );
    assert_eq!(
        early_triangle_floor(Some(ping_a), server_b, Some(219_170)),
        None
    );
    // The header/ledger metric reads through the same key.
    assert!(ping_for_server(Some(ping_a), server_b).is_none());
    assert!(
        ping_for_server(Some(ping_a), server_a).is_some(),
        "the same record is still valid evidence for its own server"
    );
    assert_eq!(direct_us_for(None, server_b), None);
}

#[test]
fn a_matching_servers_ping_is_used_exactly_as_before() {
    let server_a = std::net::Ipv4Addr::new(91, 199, 82, 162);
    let ping_a = ping_for([91, 199, 82, 162], 279.474, 12);

    assert!(ping_for_server(Some(ping_a), server_a).is_some());
    assert_eq!(direct_us_for(Some(ping_a), server_a), Some(279_474));
    let claim = direct_claim_label(direct_us_for(Some(ping_a), server_a), 279_500);
    assert!(
        claim.contains("not better than direct (+0 ms, direct 279 ms)"),
        "{claim}"
    );
    // 279.474 - 219.170 = 60.304 ms floor, identical to the task-24 arithmetic.
    assert_eq!(
        early_triangle_floor(Some(ping_a), server_a, Some(219_170)),
        Some(60_304)
    );
    assert_eq!(
        relay_cannot_beat_direct(Some(305_840), Some(ping_a), server_a),
        Some((305_840, 279_474))
    );
}

#[test]
fn the_observed_poll_ordering_never_reuses_the_previous_servers_ping() {
    // The production order inside one GUI frame: `follow_game_server` (ranking)
    // runs before `poll_relay_health` (refresh). Model both steps with the pure
    // helpers so the seam is pinned.
    let server_a = std::net::Ipv4Addr::new(91, 199, 82, 162);
    let server_b = std::net::Ipv4Addr::new(91, 199, 82, 221);

    // 1. An earlier poll refreshed the record for server A.
    let record = refreshed_server_ping(server_a, Some((279.474, 12)));
    // 2. The game moves to server B; ranking runs first, while the record is
    //    still A's. It must read "not measured" for B, not A's 279 ms.
    assert_eq!(direct_us_for(record, server_b), None);
    assert_eq!(early_triangle_floor(record, server_b, Some(219_170)), None);
    assert_eq!(
        direct_claim_label(direct_us_for(record, server_b), 286_000),
        ", direct ping not measured"
    );
    // 3. `poll_relay_health` then refreshes for B before B's burst returns:
    //    the record is replaced by None, so A's value is gone, not reused.
    let record = refreshed_server_ping(server_b, None);
    assert_eq!(direct_us_for(record, server_b), None);
    assert_eq!(direct_us_for(record, server_a), None);
    // 4. B's own ping lands and is used.
    let record = refreshed_server_ping(server_b, Some((281.0, 4)));
    assert_eq!(direct_us_for(record, server_b), Some(281_000));
    assert_eq!(direct_us_for(record, server_a), None);
}

#[test]
fn the_early_bound_refuses_a_foreign_ping() {
    use lightspeed_client::DestinationLegSource;
    let server_a = std::net::Ipv4Addr::new(91, 199, 82, 162);
    let server_b = std::net::Ipv4Addr::new(91, 199, 82, 221);
    let ping_a = ping_for([91, 199, 82, 162], 279.474, 12);
    let prior = scored_relay(
        [10, 0, 0, 2],
        219_000,
        41_000,
        DestinationLegSource::RegionPrior,
        0,
    );

    // A fresh ping from the previous server must leave the row on the prior...
    let foreign_floor = early_triangle_floor(Some(ping_a), server_b, Some(219_000));
    assert_eq!(foreign_floor, None);
    assert_eq!(
        server_leg_state(Some(&prior), None, foreign_floor),
        ServerLegState::Prior { prior_us: 41_000 }
    );
    // ...while the same ping for its own server proves the bound.
    let own_floor = early_triangle_floor(Some(ping_a), server_a, Some(219_000));
    assert_eq!(own_floor, Some(60_474));
    assert_eq!(
        server_leg_state(Some(&prior), None, own_floor),
        ServerLegState::Bounded { floor_us: 60_474 }
    );
}

#[test]
fn a_better_than_direct_claim_needs_a_measured_comparison() {
    // Nothing is claimed without a direct measurement.
    assert_eq!(compare_to_direct(283_470, None), None);
    assert!(direct_claim_label(None, 250_000).contains("not measured"));
    // Arithmetic on the two figures, in both directions and with its delta.
    assert_eq!(
        compare_to_direct(250_000, Some(283_474)),
        Some((true, 33_474))
    );
    assert_eq!(
        compare_to_direct(368_840, Some(283_474)),
        Some((false, 85_366))
    );
    assert!(direct_claim_label(Some(283_474), 250_000).contains("better than direct by 33 ms"));
    assert!(
        direct_claim_label(Some(283_474), 368_840).contains("not better than direct"),
        "a relay whose estimate exceeds direct must not be presented as better"
    );
}

#[test]
fn a_direct_ping_is_evidence_only_while_fresh() {
    assert_eq!(fresh_direct_us(Some((283.47, 0))), Some(283_470));
    assert_eq!(
        fresh_direct_us(Some((283.47, SERVER_PING_FRESH_SECS))),
        Some(283_470)
    );
    assert_eq!(
        fresh_direct_us(Some((283.47, SERVER_PING_FRESH_SECS + 1))),
        None,
        "a stale ping must not floor the ranking or support a verdict"
    );
    assert_eq!(fresh_direct_us(Some((0.0, 1))), None);
    assert_eq!(fresh_direct_us(None), None);
}

#[test]
fn the_choice_trace_names_how_the_server_leg_was_obtained() {
    use lightspeed_client::DestinationLegSource;
    let relay = addr("207.246.106.36:4434");
    let server = std::net::Ipv4Addr::new(91, 199, 82, 166);
    assert_eq!(
        ranking_state_label(DestinationLegSource::RegionPrior, relay, server),
        format!("RegionPrior(±{} ms)", PRIOR_UNCERTAINTY_US / 1000)
    );
    assert_eq!(
        ranking_state_label(DestinationLegSource::GeometricFloor, relay, server),
        "GeometricFloor"
    );
    assert_eq!(
        ranking_state_label(DestinationLegSource::Unknown, relay, server),
        "Unknown"
    );
    // No estimator samples are fed in the test process, so this is the
    // "measured but no count" arm; the prefix is the contract either way.
    assert!(
        ranking_state_label(DestinationLegSource::Measured, relay, server).starts_with("Measured")
    );
}

#[test]
fn the_surface_describes_the_relay_in_use_not_the_winner() {
    use lightspeed_client::DestinationLegSource;
    let winner = scored_relay(
        [207, 246, 124, 117],
        100_000,
        50_000,
        DestinationLegSource::Measured,
        0,
    );
    let running = scored_relay(
        [207, 246, 106, 36],
        200_000,
        40_000,
        DestinationLegSource::RegionPrior,
        0,
    );
    let scored = vec![winner, running.clone()];
    assert_eq!(
        scored_for_relay(&scored, running.relay),
        Some(&running),
        "the row must describe the relay carrying traffic, not the staged winner"
    );
    assert_eq!(scored_for_relay(&scored, addr("10.0.0.9:4434")), None);
}

#[test]
fn discovery_failure_keeps_existing_relays_and_reports_the_reason() {
    let existing = vec![
        ProxyEntry::discovered("relay-lax-1", addr("207.246.106.36:4434"), "LAX"),
        ProxyEntry::custom(addr("127.0.0.1:4434"), "Local proxy"),
    ];
    let (proxies, error) = apply_discovery_result(
        &existing,
        &DiscoveryOutcome::Failed("registry unreachable".to_string()),
    );
    assert_eq!(proxies, existing);
    assert_eq!(error.as_deref(), Some("registry unreachable"));
}

#[test]
fn discovery_success_refreshes_relays_and_keeps_custom_entries() {
    let existing = vec![
        ProxyEntry::discovered("relay-old", addr("9.9.9.9:4434"), "Old relay"),
        ProxyEntry::custom(addr("127.0.0.1:4434"), "Local proxy"),
    ];
    let outcome = DiscoveryOutcome::Found(vec![RelayInfo {
        node_id: "relay-lax-1".to_string(),
        addr: addr("207.246.106.36:4434"),
    }]);
    let (proxies, error) = apply_discovery_result(&existing, &outcome);
    assert_eq!(error, None);
    assert_eq!(proxies.len(), 2);
    assert_eq!(proxies[0].node_id.as_deref(), Some("relay-lax-1"));
    assert_eq!(proxies[0].label, "LAX - Los Angeles");
    assert_eq!(proxies[1].label, "Local proxy");
    assert!(!proxies[1].is_discovered());
}

#[test]
fn auto_mode_starts_the_detected_game_not_the_first_entry() {
    // The GUI can start before the game, so the picker's index is fixed at
    // construction and falls back to the first entry. Auto mode must follow
    // detection instead, or the zero-click start runs the wrong profile.
    let detected = super::pick_game_entry(true, Some("arcraiders"), 0);
    assert_eq!(detected.key, "arcraiders");

    let manual = super::pick_game_entry(false, Some("arcraiders"), 0);
    assert_ne!(manual.key, "arcraiders");

    let absent = super::pick_game_entry(true, None, 0);
    assert_eq!(absent.key, super::games()[0].key);
}

#[test]
fn the_relayed_row_counts_from_where_the_session_started() {
    assert_eq!(super::session_delta(180, 95, Some((100, 50))), (80, 45));
    assert_eq!(super::session_delta(180, 95, None), (180, 95));
    assert_eq!(
        super::session_delta(5, 3, Some((100, 50))),
        (0, 0),
        "counters that restarted below the baseline must not wrap"
    );
}

#[test]
fn an_interval_that_forwarded_nothing_records_nothing() {
    assert_eq!(super::history_delta((100, 40), (100, 41)), None);
    assert_eq!(
        super::history_delta((100, 40), (130, 44)),
        Some((30, 4)),
        "deltas, not totals: the engine counters are cumulative"
    );
    assert_eq!(
        super::history_delta((100, 40), (5, 3)),
        None,
        "counters that restarted produce no forwards, so the interval is skipped rather than recorded as a wraparound"
    );
}

#[test]
fn the_relayed_row_reads_the_interceptor_too() {
    // Issue #154: a process-first session showed "start optimizing to count"
    // while it was relaying, because the row only watched redirect mode.
    use super::routed_count;
    assert_eq!(routed_count(false, 0, true, 32), Some(32));
    assert_eq!(routed_count(true, 7, true, 32), Some(7));
    assert_eq!(routed_count(false, 0, false, 0), None);
}

#[test]
fn a_sparse_window_is_not_starvation() {
    // The live failure: 7 packets, 1 reply, called starving while the
    // session's own total was 187 replies for 309 sends.
    assert_eq!(
        super::relay_reply_fault(super::RELAY_REPLY_TIMEOUT, 7, 0, 1, None),
        None,
        "a seven-packet window is too small to judge"
    );
    assert_eq!(
        super::relay_reply_fault(super::RELAY_REPLY_TIMEOUT, 40, 0, 0, None),
        Some(super::RelayReplyFault::NoReplyFromRelay),
        "a real sample with no replies at all is still a warning window"
    );
}

#[test]
fn one_returned_datagram_is_not_an_injection_fault() {
    let elapsed = super::RELAY_REPLY_TIMEOUT;
    assert_eq!(
        super::relay_reply_fault(elapsed, 60, 0, 1, None),
        Some(super::RelayReplyFault::TooFewReplies),
        "a single reply is a quiet relay, not a broken injector"
    );
    assert_eq!(
        super::relay_reply_fault(elapsed, 600, 0, 40, None),
        Some(super::RelayReplyFault::InjectionFailed),
        "a run of returns with zero injections is the injector"
    );
}

#[test]
fn a_hold_is_normal_and_never_a_starvation_warning() {
    // The only hold left is the start-of-match wait for a relay choice: the
    // engine keeps traffic direct until a relay is chosen for the locked
    // server. It is not a fault, so the watchdog plan for a windowless hold
    // changes nothing, and the tray reports the connection state (boosting is
    // false until the hold lifts).
    let server = addr("198.51.100.23:36703");
    let plan = plan_return_path_window(None, None, None, server, None);
    assert_eq!(plan.event, WindowEvent::None);
    assert_eq!(plan.warning, None);
    assert!(!plan.publish_verdict);
    assert!(!plan.switch_to_direct);
    assert_eq!(
        tray_state_for(false, false, false, true),
        TrayState::Connected,
        "a held session is connected, not optimizing"
    );
}

#[test]
fn the_game_caption_never_contradicts_a_pinned_game() {
    assert_eq!(
        super::game_caption(true, false, None),
        Some("game.none_detected_hint"),
        "auto mode with nothing running is the one case that needs the hint"
    );
    assert_eq!(
        super::game_caption(false, false, None),
        None,
        "a pinned game is named in the dropdown; a 'no game' caption fights it"
    );
    assert_eq!(super::game_caption(true, false, Some("robrun")), None);
    assert_eq!(
        super::game_caption(true, true, None),
        None,
        "manual mode names its server elsewhere"
    );
}

/// Discussion #146's first ask, closed end to end: the "Custom Port Range"
/// the user typed in Manual mode is persisted, and once restored it
/// actuates - it is parsed and handed to the interceptor as the config's
/// `port_range`, beating the profile's own range.
///
/// The two steps mirror `start_boost` exactly. The assertion is about
/// reaching `InterceptorConfig`, so it lives in this file (the one that
/// wires the stored string into the engine) rather than next to the TOML
/// round-trips in `config.rs`.
#[test]
fn a_stored_manual_port_range_reaches_the_interceptor_config() {
    // As the loader restores it from disk.
    let saved = crate::config::AppConfig {
        manual_port_range: "27000-27050".to_string(),
        ..Default::default()
    };

    let game = lightspeed_client::games::resolve("rust").expect("rust is a built-in key");
    let proxy = "127.0.0.1:4434".parse().unwrap();

    let cfg = lightspeed_client::interceptor::build_config_for_game_with_port_range(
        game.as_ref(),
        proxy,
        false,
        4,
        Default::default(),
        super::parse_custom_port_range(&saved.manual_port_range),
    )
    .expect("a config is built even when the game process is not running");
    assert_eq!(
        cfg.port_range,
        (27000, 27050),
        "the stored Manual range must reach the interceptor config"
    );

    // A malformed stored value parses to `None`: the profile default
    // stands and nothing panics.
    let cfg = lightspeed_client::interceptor::build_config_for_game_with_port_range(
        game.as_ref(),
        proxy,
        false,
        4,
        Default::default(),
        super::parse_custom_port_range("not-a-range"),
    )
    .expect("a config is built even when the game process is not running");
    assert_eq!(cfg.port_range, game.ports());
}

/// The Game dropdown's model, not its pixels: the two modes, every built-in
/// in registry order, then the user's own group, then the way in.
#[test]
fn the_my_games_group_sits_below_the_built_ins_and_ends_with_the_editor_entry() {
    let built_ins = games().len();
    let rows = game_pick_rows(built_ins, 2);
    assert_eq!(rows[0], GamePickRow::Auto);
    assert_eq!(rows[1], GamePickRow::Manual);

    let builtin_at: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter_map(|(at, row)| matches!(row, GamePickRow::Builtin(_)).then_some(at))
        .collect();
    assert_eq!(
        builtin_at,
        (2..built_ins + 2).collect::<Vec<_>>(),
        "every built-in is present, in registry order, right below the modes"
    );
    for (at, row) in rows.iter().enumerate() {
        if let GamePickRow::Builtin(i) = row {
            assert_eq!(*i, at - 2, "built-in indices must not shift");
        }
    }

    let header = rows
        .iter()
        .position(|row| *row == GamePickRow::MyGamesHeader)
        .expect("the group header is drawn when profiles exist");
    assert!(
        builtin_at.iter().all(|at| *at < header),
        "the My games group sits below the built-ins: {rows:?}"
    );
    assert_eq!(
        rows.iter()
            .filter_map(|row| match row {
                GamePickRow::User(i) => Some(*i),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec![0, 1],
        "one row per profile, in store order"
    );
    assert_eq!(rows.last(), Some(&GamePickRow::NewCustom));

    // No profiles, no group - and the editor entry stays reachable.
    let rows = game_pick_rows(built_ins, 0);
    assert!(!rows.contains(&GamePickRow::MyGamesHeader));
    assert_eq!(rows.last(), Some(&GamePickRow::NewCustom));
    assert_eq!(rows.len(), built_ins + 3);
}

/// The editor's live verdict against the authority that actually refuses a
/// save: for every draft, "the Save button is enabled" must equal "the
/// schema accepts the document the button would write".
#[test]
fn the_editor_verdict_mirrors_the_schema_save_rules() {
    use lightspeed_client::profiles::schema::validate_for_save;
    use lightspeed_client::profiles::UserGameProfile;

    fn draft(name: &str, key: &str, processes: &str, udp: &str) -> GameEditorDraft {
        GameEditorDraft {
            key: key.to_string(),
            name: name.to_string(),
            process_names: processes.to_string(),
            udp_range: udp.to_string(),
            ..Default::default()
        }
    }

    fn existing(key: &str) -> UserGameProfile {
        UserGameProfile {
            name: key.to_string(),
            key: key.to_string(),
            process_names: vec!["Mine.exe".to_string()],
            udp_ports: None,
            dynamic_server: false,
            notes: None,
        }
    }

    /// What the sheet's Save button would write.
    fn document(
        existing: &[UserGameProfile],
        draft: &GameEditorDraft,
        editing: Option<usize>,
    ) -> Vec<UserGameProfile> {
        let mut games = existing.to_vec();
        match editing {
            Some(i) if i < games.len() => games[i] = draft.to_profile(),
            _ => games.push(draft.to_profile()),
        }
        games
    }

    let cases: Vec<(&str, GameEditorDraft, Option<usize>, Vec<UserGameProfile>)> = vec![
        (
            "a new profile with process names",
            draft("My Game", "", "MyGame.exe", ""),
            None,
            vec![],
        ),
        (
            "a new profile with only a UDP range",
            draft("Ports Only", "", "", "27015-27050"),
            None,
            vec![],
        ),
        (
            "a blank name",
            draft("", "", "MyGame.exe", ""),
            None,
            vec![],
        ),
        (
            "neither process names nor a range",
            draft("Empty", "", "", ""),
            None,
            vec![],
        ),
        (
            "a key a built-in claims",
            draft("Sneaky", "minecraft", "MyGame.exe", ""),
            None,
            vec![],
        ),
        (
            "a process name a built-in claims",
            draft("Sneaky", "", "RustClient.exe", ""),
            None,
            vec![],
        ),
        (
            "a key another profile already uses",
            draft("Sneaky", "my-game", "MyGame.exe", ""),
            None,
            vec![existing("my-game")],
        ),
        (
            "editing a profile keeps its own key",
            draft("Renamed", "my-game", "MyGame.exe", ""),
            Some(0),
            vec![existing("my-game")],
        ),
        (
            "a range that is not a range",
            draft("Typo", "", "", "27015-abc"),
            None,
            vec![],
        ),
        (
            "a range whose lo is above its hi",
            draft("Backwards", "", "", "27050-27015"),
            None,
            vec![],
        ),
    ];

    for (label, draft, editing, stored) in cases {
        let keys: Vec<String> = stored.iter().map(|game| game.key.clone()).collect();
        let editing_key = editing
            .and_then(|i| stored.get(i))
            .map(|game| game.key.clone());
        let verdict = validate_draft(&draft, &keys, editing_key.as_deref());
        let document = document(&stored, &draft, editing);
        let accepted = validate_for_save(&document, &[]).is_ok();
        assert_eq!(
            verdict.ok(),
            accepted,
            "{label}: the editor and the schema disagree ({verdict:?}, {document:?})"
        );
    }
}

/// A range the schema would silently drop still blocks Save: the field has
/// to actuate, not vanish (the dead-control rule this round exists to fix).
#[test]
fn a_malformed_range_blocks_saving_even_though_the_schema_would_drop_it() {
    let draft = GameEditorDraft {
        name: "Typo".to_string(),
        process_names: "MyGame.exe".to_string(),
        udp_range: "27o15-27050".to_string(),
        ..Default::default()
    };
    let verdict = validate_draft(&draft, &[], None);
    assert!(!verdict.udp_ok, "the range field is the field at fault");
    assert!(!verdict.ok(), "an unparseable range must block the save");

    // The schema alone would drop the range and save a profile without it.
    let document = vec![draft.to_profile()];
    assert!(lightspeed_client::profiles::schema::validate_for_save(&document, &[]).is_ok());
    assert_eq!(document[0].udp_ports, None);

    // And the reason shown is the range, in the user's language.
    let _guard = crate::i18n::test_lock();
    crate::i18n::set_language(Some("en"));
    let reasons = draft_reasons(&verdict, draft.key().as_deref());
    crate::i18n::set_language(None);
    assert!(
        reasons.iter().any(|reason| reason.contains("range")),
        "the range must be named: {reasons:?}"
    );
}

/// The Manual flow's "Save as…" writes a `[[servers]]` entry that survives
/// the round trip into the next start, and a game saved from the editor
/// lives in the same document without displacing it.
#[test]
fn a_saved_server_round_trips_through_the_profile_store() {
    use lightspeed_client::profiles::ProfileStore;

    let dir = std::env::temp_dir().join(format!("lightspeed-gui-profiles-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("profiles.toml");
    let store = ProfileStore::load_from(&path);

    let server = server_from_fields("My server", "203.0.113.7:27015", "27015-27050")
        .expect("a named, well-formed server");
    assert_eq!(server.address, "203.0.113.7:27015");
    assert_eq!(
        server.udp_ports.map(|range| range.to_string()).as_deref(),
        Some("27015-27050")
    );

    // Exactly what the Save as button does: read the loaded list, append,
    // write through the store.
    let mut servers = store.servers();
    servers.push(server.clone());
    save_user_profiles(&store, &store.games(), &servers).expect("save the servers");

    // The store reloaded as part of the save, so the next resolve in this
    // process sees it without a restart.
    assert_eq!(store.servers(), vec![server.clone()]);

    // The editor's own write goes through the same document, and must not
    // displace the saved server.
    let game = GameEditorDraft {
        name: "My Game".to_string(),
        process_names: "MyGame.exe".to_string(),
        ..Default::default()
    }
    .to_profile();
    save_user_profiles(&store, std::slice::from_ref(&game), &servers).expect("save both halves");

    // A fresh store is the next start: it reads both entries back.
    let reopened = ProfileStore::load_from(&path);
    assert_eq!(reopened.games(), vec![game]);
    assert_eq!(reopened.servers(), vec![server]);
    let text = std::fs::read_to_string(&path).expect("the document was written");
    assert!(text.contains("[[servers]]"), "{text}");
    assert!(text.contains("27015-27050"), "{text}");

    // An unnamed or unaddressable server is refused before anything is
    // written - the fields on screen are the fields that get saved.
    let _guard = crate::i18n::test_lock();
    crate::i18n::set_language(Some("en"));
    let unnamed = server_from_fields("  ", "203.0.113.7:27015", "");
    let unaddressed = server_from_fields("Named", "not-an-address", "");
    let bad_range = server_from_fields("Named", "203.0.113.7:27015", "nope");
    crate::i18n::set_language(None);
    assert!(unnamed.is_err() && unaddressed.is_err() && bad_range.is_err());
    assert_eq!(reopened.servers().len(), 1, "a refusal writes nothing");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The picker's one state machine: while a load is in flight - or has not
/// been started yet - the sheet says so; only a finished enumeration shows
/// rows.
#[test]
fn the_picker_reports_a_load_in_flight_and_a_finished_table() {
    let (tx, rx) = std::sync::mpsc::channel();
    assert_eq!(
        picker_status(&ProcessListState::Idle),
        PickerStatus::Loading
    );
    assert_eq!(
        picker_status(&ProcessListState::InFlight(rx)),
        PickerStatus::Loading
    );
    assert_eq!(picker_status(&ProcessListState::Done), PickerStatus::Ready);
    drop(tx);
}

/// The editor sheet, rendered and read back.
///
/// Asserting on the widget calls would pass with the sheet wired to nothing,
/// so this drives the body the sheet itself draws and collects the text it
/// actually painted: the five fields, the process picker and its rows, the
/// key that `--game <key>` takes, and the actions - with no TCP field among
/// them.
#[test]
fn the_game_editor_sheet_draws_every_field_and_the_key() {
    use eframe::egui;

    fn collect(shape: &egui::Shape, out: &mut Vec<(String, f32)>) {
        match shape {
            egui::Shape::Text(text) => out.push((
                text.galley.text().to_string(),
                text.pos.x + text.galley.size().x,
            )),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, out);
                }
            }
            _ => {}
        }
    }

    let _guard = crate::i18n::test_lock();
    crate::i18n::set_language(Some("en"));

    let mut draft = GameEditorDraft::new_profile();
    draft.name = "My Game".to_string();
    draft.process_names = "MyGame.exe".to_string();
    draft.udp_range = "27015-27050".to_string();
    let verdict = validate_draft(&draft, &[], None);
    assert!(
        verdict.ok(),
        "the fixture must be a valid draft: {verdict:?}"
    );

    // One picker row, with a path far wider than the sheet: the row has to
    // be drawn, trimmed, and still stay inside the sheet (whose cap at a
    // 440 px screen is `sheet_bounds`' 416).
    let rows = vec![ProcessEntry {
        pid: 4242,
        name: "MyGame.exe".to_string(),
        path: Some(
            "C:\\Program Files\\Some Vendor\\A Very Long Game Folder\\bin\\win64\\MyGame.exe"
                .to_string(),
        ),
        started_at: None,
    }];

    let ctx = egui::Context::default();
    super::install_fonts(&ctx, Some("en"));
    super::apply_theme(&ctx);
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(440.0, 800.0),
        )),
        ..Default::default()
    };
    let mut search = String::new();
    let mut actions = EditorActions::default();
    let mut out = ctx.run_ui(raw, |ui| {
        game_editor_body(
            ui,
            &mut draft,
            &verdict,
            None,
            None,
            &[],
            ProcessPicker {
                rows: &rows,
                status: PickerStatus::Ready,
                search: &mut search,
            },
            &mut actions,
        );
    });
    out.textures_delta.clear();
    let mut drawn = Vec::new();
    for clipped in &out.shapes {
        collect(&clipped.shape, &mut drawn);
    }
    let frame = drawn
        .iter()
        .map(|(text, _)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let widest = drawn
        .iter()
        .map(|(_, right)| *right)
        .fold(0.0_f32, f32::max);
    crate::i18n::set_language(None);

    // Drawn, and drawn in order: this is the sheet's vertical layout, top
    // to bottom, read back off the frame.
    let mut last = 0;
    for expected in [
        "My games",
        "Name:",
        "Process names:",
        "Running processes",
        "Refresh",
        "Filter by name or path",
        "UDP range:",
        "Server rotates between matches",
        "Notes:",
        "Key: my-game - start the CLI with --game my-game",
        "Save game",
    ] {
        let at = frame
            .find(expected)
            .unwrap_or_else(|| panic!("the sheet did not draw {expected:?}:\n{frame}"));
        assert!(at >= last, "{expected:?} is drawn out of order:\n{frame}");
        last = at;
    }
    // The picker row itself: the image name, the PID, and a path trimmed to
    // its tail - the tail has to reach the frame, not the full path.
    assert!(
        frame.contains("MyGame.exe  4242  …bin\\win64\\MyGame.exe"),
        "the picker did not draw the row's name, PID and trimmed path:\n{frame}"
    );
    assert!(
        widest <= 416.0,
        "a row was painted past the sheet's width ({widest} px):\n{frame}"
    );
    assert!(
        !frame.contains("TCP"),
        "the editor must not offer a TCP field:\n{frame}"
    );
    assert_eq!(actions, EditorActions::default(), "a render clicks nothing");
}

/// The pick the front door makes is the state the primary action starts
/// interception from: the chosen row becomes the target, the engine binds it
/// by image name against the live process table, and the config the
/// interceptor starts from carries that process - not a port range.
#[test]
fn a_picked_process_becomes_the_target_interception_starts_from() {
    use lightspeed_client::interceptor::traits::ProcessInfo;
    use lightspeed_client::interceptor::{build_config_for_process_target, resolve_process_target};

    let entry = ProcessEntry {
        pid: 4242,
        name: "MyGame.exe".to_string(),
        path: Some("C:\\Games\\MyGame\\MyGame.exe".to_string()),
        started_at: None,
    };
    let target = apply_target_choice(None, Some(target_for_pick(&entry)), false)
        .expect("choosing a row sets the target");

    // The status surface and the start path agree on the identity.
    assert_eq!(target.describe(), "MyGame.exe (PID 4242)");

    // What `start_boost` does with it: bind the name to the live table,
    // then build the config the interceptor starts from.
    let live = ProcessInfo {
        pid: 4242,
        name: "MyGame.exe".to_string(),
        routes: vec![],
    };
    let bound = resolve_process_target(&target, Some(&live)).expect("the running process binds");
    let game = lightspeed_client::games::resolve("rust").expect("rust is a built-in key");
    let cfg = build_config_for_process_target(
        game.as_ref(),
        bound.clone(),
        vec![],
        addr("127.0.0.1:4434"),
        false,
        4,
        Default::default(),
    );
    assert_eq!(
        cfg.process_target,
        Some(bound),
        "the config the primary action starts from must carry the picked process"
    );
    assert_eq!(
        cfg.process_names,
        vec!["MyGame.exe".to_string()],
        "the config targets the picked image, not the profile's own processes"
    );
}

/// Clearing drops the target, and the built-in Game path is exactly what the
/// primary action builds next: a config with no process target and the
/// profile's own port range standing again.
#[test]
fn clearing_the_target_returns_to_the_built_in_game_path() {
    let target = ProcessTarget {
        name: Some("MyGame.exe".to_string()),
        pid: Some(4242),
    };
    assert_eq!(
        apply_target_choice(Some(target.clone()), None, true),
        None,
        "Clear drops the target"
    );
    assert_eq!(
        apply_target_choice(Some(target.clone()), None, false),
        Some(target),
        "a frame that neither picks nor clears leaves the target alone"
    );

    let game = lightspeed_client::games::resolve("rust").expect("rust is a built-in key");
    let cfg = lightspeed_client::interceptor::build_config_for_game_with_port_range(
        game.as_ref(),
        addr("127.0.0.1:4434"),
        false,
        4,
        Default::default(),
        None,
    )
    .expect("the built-in path builds a config");
    assert_eq!(
        cfg.process_target, None,
        "the built-in path carries no process target"
    );
    assert_eq!(
        cfg.port_range,
        game.ports(),
        "the profile's own range decides again"
    );
}

/// The status surface's line, through i18n: the same identity in English and
/// German, with the placeholder substituted and never left as a brace.
#[test]
fn the_target_status_line_names_the_process_and_its_pid() {
    let _guard = crate::i18n::test_lock();
    let target = ProcessTarget {
        name: Some("MyGame.exe".to_string()),
        pid: Some(4242),
    };
    crate::i18n::set_language(Some("en"));
    assert_eq!(target_summary(&target), "Target: MyGame.exe (PID 4242)");
    crate::i18n::set_language(Some("de"));
    assert_eq!(target_summary(&target), "Ziel: MyGame.exe (PID 4242)");
    crate::i18n::set_language(None);
}

/// The front door, rendered and read back: the picked process and the way
/// back to the built-in path, or the way in to the picker - inside a 320 px
/// window, with nothing clicked.
#[test]
fn the_target_front_door_draws_the_pick_and_the_way_back() {
    use eframe::egui;

    fn collect(shape: &egui::Shape, out: &mut Vec<(String, f32)>) {
        match shape {
            egui::Shape::Text(text) => out.push((
                text.galley.text().to_string(),
                text.pos.x + text.galley.size().x,
            )),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, out);
                }
            }
            _ => {}
        }
    }

    let _guard = crate::i18n::test_lock();
    crate::i18n::set_language(Some("en"));
    let target = ProcessTarget {
        name: Some("MyGame.exe".to_string()),
        pid: Some(4242),
    };

    let frame = |target: Option<&ProcessTarget>| -> (String, f32) {
        let ctx = egui::Context::default();
        super::install_fonts(&ctx, Some("en"));
        super::apply_theme(&ctx);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(320.0, 800.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            let action = target_front_door(ui, target);
            assert_eq!(
                action,
                TargetRowAction::default(),
                "a render clicks nothing"
            );
        });
        out.textures_delta.clear();
        let mut drawn = Vec::new();
        for clipped in &out.shapes {
            collect(&clipped.shape, &mut drawn);
        }
        let widest = drawn
            .iter()
            .map(|(_, right)| *right)
            .fold(0.0_f32, f32::max);
        (
            drawn
                .iter()
                .map(|(text, _)| text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            widest,
        )
    };

    let (set, set_wide) = frame(Some(&target));
    assert!(
        set.contains("Target: MyGame.exe (PID 4242)"),
        "the front door did not name the picked process:\n{set}"
    );
    assert!(
        set.contains("Clear target"),
        "the front door did not offer the way back:\n{set}"
    );
    assert!(
        set_wide <= 320.0,
        "the front door drew past a 320 px window ({set_wide} px):\n{set}"
    );

    let (unset, unset_wide) = frame(None);
    assert!(
        unset.contains("Target a process"),
        "the front door did not offer the picker:\n{unset}"
    );
    assert!(
        !unset.contains(crate::i18n::t("game.target_none").as_ref()),
        "the explanation is a hover now - drawn inline it wrapped to two lines \
         at 320 px and pushed the sheet taller:\n{unset}"
    );
    assert!(
        unset_wide <= 320.0,
        "the front door drew past a 320 px window ({unset_wide} px):\n{unset}"
    );
    crate::i18n::set_language(None);
}

/// The picker both surfaces share: an in-flight load, an empty table, and a
/// filter nothing matches each say what they are, and a finished table draws
/// its rows - all without enumerating or blocking.
#[test]
fn the_shared_picker_draws_loading_empty_and_no_match_honestly() {
    use eframe::egui;

    fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.text().to_string()),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, out);
                }
            }
            _ => {}
        }
    }

    let _guard = crate::i18n::test_lock();
    crate::i18n::set_language(Some("en"));
    let rows = vec![ProcessEntry {
        pid: 7,
        name: "steam.exe".to_string(),
        path: Some("C:\\Steam\\steam.exe".to_string()),
        started_at: None,
    }];

    let draw = |status: PickerStatus, rows: &[ProcessEntry], query: &str| -> String {
        let ctx = egui::Context::default();
        super::install_fonts(&ctx, Some("en"));
        super::apply_theme(&ctx);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(440.0, 800.0),
            )),
            ..Default::default()
        };
        let mut search = query.to_string();
        let mut refresh = false;
        let mut out = ctx.run_ui(raw, |ui| {
            let mut picker = ProcessPicker {
                rows,
                status,
                search: &mut search,
            };
            let chosen = process_picker(ui, &mut picker, |_| false, &mut refresh);
            assert!(chosen.is_none(), "a render clicks no row");
        });
        out.textures_delta.clear();
        assert!(!refresh, "a render presses no button");
        let mut drawn = Vec::new();
        for clipped in &out.shapes {
            collect(&clipped.shape, &mut drawn);
        }
        drawn.join("\n")
    };

    assert!(
        draw(PickerStatus::Loading, &[], "")
            .contains(crate::i18n::t("game.editor_process_loading").as_ref()),
        "a load in flight must say so"
    );
    assert!(
        draw(PickerStatus::Ready, &[], "")
            .contains(crate::i18n::t("game.editor_process_empty").as_ref()),
        "an empty table must say so"
    );
    assert!(
        draw(PickerStatus::Ready, &rows, "nonesuch")
            .contains(crate::i18n::t("game.editor_process_no_match").as_ref()),
        "a filter nothing matches must say so"
    );
    let ready = draw(PickerStatus::Ready, &rows, "");
    assert!(
        ready.contains("steam.exe"),
        "a finished table draws its rows:\n{ready}"
    );
    assert!(ready.contains('7'), "the row names the PID:\n{ready}");
    crate::i18n::set_language(None);
}
