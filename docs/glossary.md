# LightSpeed Glossary

> **How this is used:** Every tooltip in the LightSpeed app links back to a section on this page.  
> The left column is what you see **in the app**; the right is the technical name used in code and docs.

---

## Core Concepts

| **In the App**           | **Technical Name**          | **What it means**                                                                                                                                   |
|--------------------------|-----------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------|
| Boost Server             | Proxy node                  | A LightSpeed relay server in a major city. Your game packets travel through this server instead of the slow default route.                           |
| Boost Ping               | Proxy RTT                   | The round-trip time (in milliseconds) from your PC to the Boost Server. Lower = faster.                                                             |
| Boost Engaged / BOOSTED  | WinDivert active            | The kernel-level intercept is running. Your game's connection is fully routed through the Boost Server.                                              |
| Finding your game server | Auto-detection / debounce   | LightSpeed watches your outbound UDP packets and identifies the game server IP automatically — no manual config needed.                              |
| Packets Sent             | packets_intercepted         | Game packets captured at the OS level and forwarded to the Boost Server.                                                                             |
| Packets Returned         | packets_from_proxy          | Responses from the Boost Server (which it relayed from the game server).                                                                             |
| Packets Delivered        | packets_injected            | Spoofed responses injected back to your game client so it thinks they came directly from the game server.                                            |
| Lost packets recovered   | FEC recovered               | Forward Error Correction rebuilt a packet that was dropped in transit — your game never noticed.                                                     |
| Reliability Shield       | FEC (Forward Error Correction) | An optional mode that sends extra redundancy data (+25% bandwidth) so the Boost Server can reconstruct dropped packets before they hurt your gameplay. |
| Deep Boost               | Active redirect / WinDivert | The strongest mode — intercepts packets at the Windows kernel level. Requires Administrator. Used automatically when available.                       |
| Heartbeat                | Keepalive                   | A tiny "are you still there?" message sent every 5 seconds between your app and the Boost Server to measure latency and keep the connection alive.   |
| Drops                    | Inject errors               | Packets that couldn't be delivered back to your game — usually a sign of a driver permission issue.                                                  |
| Server Switch            | Re-detection                | When you disconnect from one game server and join another, LightSpeed automatically detects the new server within ~5 seconds.                       |

---

## Status Indicators

| **Indicator**        | **Meaning**                                                                                             |
|----------------------|---------------------------------------------------------------------------------------------------------|
| ● Green              | Connected to Boost Server and all systems working.                                                      |
| ● Yellow / Amber     | Connected to Boost Server but boost not yet active (waiting for game to launch).                        |
| ● Red                | Disconnected from Boost Server or an error occurred. Check your internet connection.                    |
| ⚡ Yellow bolt (tray) | Boost is engaged — game traffic is being optimised.                                                     |
| ⚡ Grey bolt (tray)  | Disconnected — LightSpeed is running in the background but not active.                                  |
| ⚡ Green bolt (tray) | Actively optimising game traffic.                                                                        |
| ⚡ Red bolt (tray)   | Error state — click the tray icon to open the app and see the error message.                            |

---

## Ping / Latency Colours

| **Colour** | **Range**    | **What it means**                 |
|------------|--------------|-----------------------------------|
| 🟢 Green   | < 60 ms      | Excellent — you won't notice lag. |
| 🟡 Yellow  | 60 – 120 ms  | Acceptable for most games.        |
| 🔴 Red     | > 120 ms     | High — may cause rubber-banding.  |

---

## Modes

| **Mode**        | **When it's used**                                     | **Requires**                        |
|-----------------|-------------------------------------------------------|--------------------------------------|
| Deep Boost (WinDivert) | Default when running as Administrator.         | Admin rights, WinDivert64.sys        |
| Manual Boost    | Advanced panel — you type the server IP:port yourself. | Nothing (no Admin needed)           |
| Reliability Shield | Add-on to either mode. Optional, uses +25% data.  | Nothing extra                        |

---

*Last updated: 2026-05 · [Back to docs index](../docs/README.md) · [Wiki home](https://github.com/ShibbityShwab/lightspeed/wiki)*
