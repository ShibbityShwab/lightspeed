//! # Minecraft Game Configuration
//!
//! Game-specific settings for Mojang's Minecraft (Microsoft).
//!
//! ## Editions and transports
//!
//! - **Bedrock Edition** relays gameplay over **UDP 19132** (IPv4) and
//!   **19133** (IPv6), so [`MinecraftConfig`] is carried directly by the
//!   tunnel's UDP path.
//! - **Java Edition** connects over **TCP 25565**. [`MinecraftJavaConfig`]
//!   marks it as a TCP game so the client terminates TCP locally and carries
//!   the stream in the v5 datagram path (see `docs/tcp-tunnel-design.md`).
//!   Java is no longer "out of scope" the way the old issue #137 note said.
//!
//! Java Edition's UDP 4445 is LAN discovery only - link-local, never routed
//! across a proxy - so it plays no part in either profile.

use super::{GameConfig, TransportProto};

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
         5. Java Edition is TCP-only and supported through the TCP path:\n\
            use --game minecraft-java (see that profile's instructions)\n\
         6. Anti-cheat: none client-side; forwarding is transparent"
            .to_string()
    }
}

/// Minecraft (Java Edition) game configuration.
///
/// Java Edition speaks TCP, so this profile reports [`TransportProto::Tcp`]
/// and a TCP port range of 25565. The tunnel terminates TCP at both ends and
/// carries the stream as sequenced datagrams rather than forwarding raw TCP
/// segments - see `docs/tcp-tunnel-design.md`.
pub struct MinecraftJavaConfig;

impl GameConfig for MinecraftJavaConfig {
    fn name(&self) -> &str {
        "Minecraft Java Edition"
    }

    fn process_names(&self) -> &[&str] {
        // Both image names are shared by every Java application - IDEs, build
        // tools, standalone servers - so the image name alone is ambiguous.
        // `command_line_marker()` supplies the discriminator that
        // auto-detection must verify before locking this profile in.
        &["javaw.exe", "java.exe"]
    }

    fn command_line_marker(&self) -> Option<&str> {
        // The Mojang launcher always passes the game's main class as
        // `--mainClass net.minecraft.client.main.Main`; no other Java app does.
        Some("net.minecraft.client.main.Main")
    }

    fn ports(&self) -> (u16, u16) {
        // Java Edition has no UDP gameplay port; its network port is the TCP
        // port reported by `tcp_ports()` below. This keeps `redirect_port()`
        // and the port diagnostics pointing at 25565.
        (25565, 25565)
    }

    fn tcp_ports(&self) -> (u16, u16) {
        (25565, 25565)
    }

    fn transport(&self) -> TransportProto {
        TransportProto::Tcp
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
        // The terminator carries TCP segments up to the path MTU; ~1.4 KB is
        // the segment-size ceiling the reliability layer is sized for.
        (64, 1400)
    }

    fn redirect_instructions(&self) -> String {
        "Minecraft (Java Edition) redirect mode:\n\
         1. Java uses TCP 25565 - LightSpeed terminates TCP and carries the\n\
            stream through the tunnel's v5 datagram path\n\
         2. Add the server you play on (default port 25565) as the game server\n\
         3. Start LightSpeed: --game minecraft-java --game-server <SERVER_IP>:25565\n\
         4. Servers on custom ports pass that port in --game-server\n\
         5. Unavailable when the tunnel falls back to its TCP transport\n\
            (TCP-in-TCP is banned)\n\
         6. Anti-cheat: none client-side; payloads are forwarded unmodified"
            .to_string()
    }
}
