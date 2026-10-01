//! # Minecraft Game Configuration
//!
//! Game-specific settings for Mojang's Minecraft (Microsoft).
//!
//! ## Scope: Bedrock Edition only
//!
//! LightSpeed's capture filter is UDP-only (`CaptureFilter` emits `udp port` /
//! `udp portrange`), and Minecraft's editions differ in transport:
//!
//! - **Bedrock Edition** relays gameplay over **UDP 19132** (IPv4) and
//!   **19133** (IPv6), so it is accelerable.
//! - **Java Edition** connects over **TCP 25565**, so multiplayer is out of
//!   scope rather than merely unoptimised. Its UDP 4445 is LAN discovery only,
//!   which is link-local and has nothing to route across a proxy - see issue
//!   #137.
//!
//! [`MinecraftConfig::process_names`] therefore omits `javaw.exe`. That name is
//! shared by every Java application - IDEs, build tools, servers - so matching
//! it would report "Minecraft" for any Java process while still being unable to
//! accelerate the game.

use super::GameConfig;

/// Minecraft (Bedrock Edition) game configuration.
pub struct MinecraftConfig;

impl GameConfig for MinecraftConfig {
    fn name(&self) -> &str {
        "Minecraft"
    }

    fn process_names(&self) -> &[&str] {
        // `Minecraft.Windows.exe` is the Store (UWP) Bedrock client and carries
        // that image name at runtime. `Minecraft.exe` and `MinecraftLauncher.exe`
        // are launcher names, listed so a launcher-only moment still resolves.
        &[
            "Minecraft.Windows.exe",
            "Minecraft.exe",
            "MinecraftLauncher.exe",
        ]
    }

    fn ports(&self) -> (u16, u16) {
        (19132, 19133)
    }

    fn anti_cheat(&self) -> &str {
        "None (server-side validation)"
    }

    fn uses_sdr(&self) -> bool {
        false
    }

    fn typical_pps(&self) -> u32 {
        20
    }

    fn packet_size_range(&self) -> (usize, usize) {
        (64, 1200)
    }

    fn redirect_instructions(&self) -> String {
        "Minecraft (Bedrock Edition) redirect mode:\n\
         1. Bedrock uses UDP 19132/19133 - the tunnel can carry it directly\n\
         2. Add the server you play on (default port 19132) as the game server\n\
         3. Start LightSpeed: --game minecraft --game-server <SERVER_IP>:19132\n\
         4. Servers on non-default ports need that port passed here\n\
         5. Java Edition is TCP-only and out of scope for the UDP tunnel (issue #137)\n\
         6. Anti-cheat: none client-side; UDP forwarding is transparent"
            .to_string()
    }
}
