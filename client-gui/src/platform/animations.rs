//! The OS reduced-motion signal (WCAG 2.2.2; spec D5 and W2-4 (i)).
//!
//! The polarity is the trap: the Windows setting means **animations enabled**
//! (`SPI_GETCLIENTAREAANIMATION` returns a non-zero `BOOL` when client-area
//! animation is on). The GUI may animate only when the OS explicitly says `On`;
//! `Off` and `Unknown` both render static text, which is the conservative
//! WCAG 2.2.2 pause/stop alternative.
//!
//! One real source is wired today:
//!
//! | OS | Source | Today |
//! |---|---|---|
//! | Windows | `SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION)` (`windows.rs`) | **R** probe, static on `Off` |
//! | macOS | none — `NSWorkspace.accessibilityDisplayShouldReduceMotion` is a gap | `Unknown` = static |
//! | Linux | none — no portable gsettings/KDE read in this crate | `Unknown` = static |
//!
//! The macOS/Linux cells are named gaps, not silent defaults: the bridge lives
//! in [`crate::platform::Platform::animations`], and each backend must answer
//! explicitly. A spinner may never appear before the OS source is wired.

/// What the OS says about motion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Animations {
    /// OS animations are explicitly enabled.
    On,
    /// Reduced motion is requested: never animate.
    Off,
    /// No OS signal is wired (or the read failed): never animate.
    Unknown,
}

impl Animations {
    /// Whether motion is allowed. Only an explicit `On` permits it.
    pub fn allows_motion(self) -> bool {
        self == Self::On
    }

    /// Map a Windows `SystemParametersInfoW` result to the signal.
    ///
    /// `call_ok` is the API's `BOOL` return (zero = the query failed);
    /// `enabled` is the value written by `SPI_GETCLIENTAREAANIMATION`, where
    /// non-zero means client-area animation is **enabled**. A failed query is
    /// `Unknown` (static), never an accidental `On`.
    pub fn from_windows_flag(call_ok: bool, enabled: i32) -> Self {
        if !call_ok {
            return Self::Unknown;
        }
        if enabled == 0 {
            Self::Off
        } else {
            Self::On
        }
    }

    /// The source behind this value, for the parity report and the log.
    pub fn source(self) -> &'static str {
        #[cfg(windows)]
        {
            "windows:SPI_GETCLIENTAREAANIMATION"
        }
        #[cfg(not(windows))]
        {
            "none:Unknown-gap"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_explicit_on_allows_motion() {
        assert!(Animations::On.allows_motion());
        assert!(!Animations::Off.allows_motion());
        // The conservative default: no source wired means no motion, so the
        // spinner can never appear on a platform whose signal is a gap.
        assert!(!Animations::Unknown.allows_motion());
    }

    #[test]
    fn the_windows_polarity_means_animations_enabled() {
        // SPI_GETCLIENTAREAANIMATION's value is "animations on" when non-zero.
        assert_eq!(Animations::from_windows_flag(true, 1), Animations::On);
        assert_eq!(Animations::from_windows_flag(true, 0), Animations::Off);
        // Any non-zero value is "on", not just 1.
        assert_eq!(Animations::from_windows_flag(true, 7), Animations::On);
    }

    #[test]
    fn a_failed_probe_is_unknown_and_never_on() {
        assert_eq!(Animations::from_windows_flag(false, 1), Animations::Unknown);
        assert_eq!(Animations::from_windows_flag(false, 0), Animations::Unknown);
    }

    #[test]
    fn off_and_unknown_are_both_static() {
        assert!(!Animations::Off.allows_motion());
        assert!(!Animations::Unknown.allows_motion());
        // The whole point of keeping them distinct: the report can name the
        // gap (`Unknown`) without ever letting it animate.
        assert_ne!(Animations::Off, Animations::Unknown);
    }
}
