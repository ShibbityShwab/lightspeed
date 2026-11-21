//! The updater install-channel decision (cross-platform plan G1.6; spec W2-4 (g)).
//!
//! The shipped Windows in-place updater downloads and verifies a zip asset and
//! hands it to an elevated swap script. The Linux and macOS builds have no such
//! asset by design (`client-gui/src/update.rs::zip_assets` is `cfg(windows)`),
//! so the sheet must **never** draw an install button there, and it must not
//! leave the user with a bare "Update available" and no way to act. This module
//! is the one decision:
//!
//! | Install | Receipt | Asset | Channel | Action |
//! |---|---|---|---|---|
//! | Windows | no | yes | `WindowsInPlace` | Install button |
//! | Windows | no | no | `WindowsNoAsset` | releases instruction |
//! | any | yes | – | `ShellInstaller` | installer/updater instruction |
//! | Linux | no | no | `PackageManager` | package-manager instruction |
//! | macOS | no | no | `BareTarball` | releases instruction |
//!
//! `install_action()` is true for exactly one channel, and every other channel
//! names a catalog key with the instruction the sheet renders. A dead install
//! button is therefore impossible by construction: the button and the action
//! come from the same decision.
//!
//! Copy keys are added in the same change. They ship in English in the eight
//! translated catalogs until the Q5 translation pipeline runs (the same
//! translation-debt rule every other wave uses; recorded in the W2-4 report).

use crate::platform::shortcuts::Os;

/// Where the running install came from, as far as the app can tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallChannel {
    /// Windows, with the in-place zip + sha256 asset resolved: the one channel
    /// that may offer the in-place Install button.
    WindowsInPlace,
    /// Windows, but the release has no matching zip/checksum pair: tell the
    /// user to use the release page, never draw a button that cannot act.
    WindowsNoAsset,
    /// A cargo-dist shell-installer receipt exists (any OS): use the installer
    /// / updater that owns this install.
    ShellInstaller,
    /// Linux without a receipt: the AUR/package-manager install.
    PackageManager,
    /// macOS without a receipt: the bare release tarball.
    BareTarball,
}

impl InstallChannel {
    /// Whether the sheet may draw its in-place Install button.
    pub fn install_action(self) -> bool {
        matches!(self, Self::WindowsInPlace)
    }

    /// The catalog key for the instruction rendered when no install action is
    /// available. `None` only on the channel that owns the Install button.
    pub fn instruction_key(self) -> Option<&'static str> {
        match self {
            Self::WindowsInPlace => None,
            Self::WindowsNoAsset | Self::BareTarball => Some("update.channel_release"),
            Self::ShellInstaller => Some("update.channel_shell"),
            Self::PackageManager => Some("update.channel_package"),
        }
    }
}

/// The channel decision. `install_asset` is whether the release resolved the
/// Windows zip + checksum pair; `receipt_present` is whether cargo-dist's
/// install receipt was found.
pub fn channel(os: Os, receipt_present: bool, install_asset: bool) -> InstallChannel {
    if install_asset {
        // Only the Windows fallback ever resolves these assets.
        return InstallChannel::WindowsInPlace;
    }
    if receipt_present {
        return InstallChannel::ShellInstaller;
    }
    match os {
        Os::Windows => InstallChannel::WindowsNoAsset,
        Os::Linux => InstallChannel::PackageManager,
        Os::Macos => InstallChannel::BareTarball,
    }
}

/// The one line the update sheet shows for this result: "Update available"
/// alone when the platform owns an install action, the channel instruction
/// appended when it does not.
pub fn available_line(channel: InstallChannel) -> String {
    let available = crate::i18n::t("update.available");
    if channel.install_action() {
        return available.into_owned();
    }
    let instruction = crate::i18n::t(
        channel
            .instruction_key()
            .unwrap_or("update.channel_release"),
    );
    format!("{available} — {instruction}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn i18n_guard() -> std::sync::MutexGuard<'static, ()> {
        let guard = crate::i18n::test_lock();
        crate::i18n::set_language(Some("en"));
        guard
    }

    #[test]
    fn only_the_windows_asset_channel_may_draw_an_install_button() {
        let channels = [
            InstallChannel::WindowsInPlace,
            InstallChannel::WindowsNoAsset,
            InstallChannel::ShellInstaller,
            InstallChannel::PackageManager,
            InstallChannel::BareTarball,
        ];
        for channel in channels {
            assert_eq!(
                channel.install_action(),
                channel == InstallChannel::WindowsInPlace,
                "{channel:?}"
            );
            // No dead button: every non-install channel names copy that says
            // what to do instead; the install channel has a real action.
            assert_eq!(
                channel.install_action(),
                channel.instruction_key().is_none()
            );
            assert!(!format!("{channel:?}").is_empty());
        }
    }

    #[test]
    fn a_resolved_asset_is_the_windows_in_place_channel() {
        for os in [Os::Windows, Os::Linux, Os::Macos] {
            assert_eq!(
                channel(os, false, true),
                InstallChannel::WindowsInPlace,
                "{os:?}: only Windows resolves the zip asset, and it must win"
            );
        }
    }

    #[test]
    fn a_receipt_owns_the_install_before_the_os_channel() {
        for os in [Os::Windows, Os::Linux, Os::Macos] {
            assert_eq!(
                channel(os, true, false),
                InstallChannel::ShellInstaller,
                "{os:?}: a shell-installer receipt means the installer owns updates"
            );
        }
    }

    #[test]
    fn without_a_receipt_each_os_names_its_own_channel() {
        assert_eq!(
            channel(Os::Windows, false, false),
            InstallChannel::WindowsNoAsset
        );
        assert_eq!(
            channel(Os::Linux, false, false),
            InstallChannel::PackageManager
        );
        assert_eq!(
            channel(Os::Macos, false, false),
            InstallChannel::BareTarball
        );
    }

    #[test]
    fn the_install_channel_shows_the_plain_available_line() {
        let _guard = i18n_guard();
        assert_eq!(
            available_line(InstallChannel::WindowsInPlace),
            "Update available"
        );
    }

    #[test]
    fn the_instruction_channels_say_what_to_do() {
        let _guard = i18n_guard();
        for (channel, marker) in [
            (InstallChannel::ShellInstaller, "installer"),
            (InstallChannel::PackageManager, "package manager"),
            (InstallChannel::BareTarball, "release"),
            (InstallChannel::WindowsNoAsset, "release"),
        ] {
            let line = available_line(channel);
            assert!(
                line.starts_with("Update available — "),
                "{channel:?} lost the result: {line}"
            );
            assert!(
                line.to_lowercase().contains(marker),
                "{channel:?} instruction does not mention {marker}: {line}"
            );
            // The instruction is copy, never a raw diagnostic.
            assert!(!line.contains("HTTP"), "{channel:?}: {line}");
        }
    }

    #[test]
    fn every_channel_key_exists_in_the_source_catalog() {
        let _guard = i18n_guard();
        let source = crate::i18n::source_keys();
        for channel in [
            InstallChannel::ShellInstaller,
            InstallChannel::PackageManager,
            InstallChannel::BareTarball,
            InstallChannel::WindowsNoAsset,
        ] {
            let key = channel.instruction_key().expect("instruction channel");
            assert!(
                source.iter().any(|existing| existing == key),
                "{key} is missing from en.toml"
            );
            assert_ne!(
                crate::i18n::t(key).as_ref(),
                key,
                "{key} has no English value"
            );
        }
    }
}
