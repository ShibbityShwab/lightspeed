//! WinDivert active redirect mode — public API mirroring `capture_mode.rs`.
//!
//! Wraps [`crate::capture::windivert_redirect::run_windivert_redirect`] with
//! the same shutdown-receiver + stat-slot pattern used by capture mode so that
//! [`crate::engine::LightSpeedEngine`] can drive it uniformly.

use std::sync::Arc;

use tokio::sync::oneshot;

use crate::capture::windivert_redirect::{WinDivertConfig, WinDivertStats};

/// Slot type passed into `run_windivert_mode_with_shutdown` so the engine
/// can read live WinDivert counters via `snapshot()`.
pub type WinDivertStatSlot = Arc<std::sync::Mutex<Option<Arc<WinDivertStats>>>>;

/// Run WinDivert redirect mode with an external shutdown oneshot and an
/// optional stat-slot that is filled once the redirect task initialises.
///
/// This is the primary entry point used by [`LightSpeedEngine::start_windivert`].
pub async fn run_windivert_mode_with_shutdown(
    cfg: WinDivertConfig,
    shutdown_rx: oneshot::Receiver<()>,
    stat_slot: Option<WinDivertStatSlot>,
) -> anyhow::Result<()> {
    let stats = Arc::new(WinDivertStats::default());

    // Fill the stat slot so the engine can poll atomics on every frame.
    if let Some(ref slot) = stat_slot {
        if let Ok(mut guard) = slot.lock() {
            *guard = Some(Arc::clone(&stats));
        }
    }

    crate::capture::windivert_redirect::run_windivert_redirect(cfg, stats, shutdown_rx).await
}
