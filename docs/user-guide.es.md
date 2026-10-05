# Guía de usuario de LightSpeed

> [!WARNING]
> Traducción asistida por máquina, no revisada por un hablante nativo. La [versión en inglés](user-guide.md) es la autoritativa.

> Instrucciones paso a paso para reducir tu ping con LightSpeed.

---

## Cómo funciona LightSpeed

Tu ISP enruta el tráfico de juegos por caminos optimizados para el coste, no para la velocidad. LightSpeed intercepta los paquetes UDP de tu juego y los tuneliza a través de un **relay** - un servidor ligero en un centro de datos con conexiones de backbone de alta velocidad hacia las regiones de los servidores de juego. Por defecto usas la **red comunitaria de relays** (ocho relays financiados por patrocinadores, descubiertos automáticamente mediante un registro firmado; no hace falta configurar nada). También puedes alojar tu propio proxy. Si ese camino es más rápido que la ruta por defecto de tu ISP, tu ping baja.

```
Your PC ──→ ISP (slow path) ──→ Game Server        ❌ High ping
Your PC ──→ LightSpeed Proxy (fast backbone) ──→ Game Server   ✅ Low ping
```

---

## Requisitos previos

- La herramienta de línea de comandos `lightspeed` o `lightspeed-gui` (Windows). No hace falta configurar ningún proxy: el cliente descubre los relays comunitarios automáticamente.
- Para el modo interceptor: privilegios de root/Administrador
- Opcional: tu propio nodo proxy si prefieres alojarlo tú mismo (consulta [Deploy Proxy](deploy-proxy.md))

---

## ¿Qué aplicación necesito?

| Estás en | Descarga | Por qué |
|-----------|----------|-----|
| **Windows** | `lightspeed-gui` (MSI o ZIP) | La GUI es una app independiente: ya incluye el motor del cliente + el driver WinDivert. **No** necesitas la CLI. |
| **Linux** | `lightspeed-gui` (o `lightspeed-client`) | La GUI funciona en Linux (la bandeja del sistema es un stub); la CLI es para usuarios avanzados. |
| **macOS** | `lightspeed-client` | Todavía no hay GUI probada. La GUI compila para macOS pero no está **probada** en hardware real. |
| **Alojar un proxy** | `lightspeed-proxy` | Solo si vas a ejecutar un nodo relay en un VPS. |

> **Nunca necesitas más de un paquete.** Si juegas en Windows, coge `lightspeed-gui` e ignora el resto. `lightspeed-client` es para usuarios avanzados de Linux y jugadores de macOS; `lightspeed-proxy` es para quien aloja su propio relay.

---

## Inicio rápido (CLI - todas las plataformas)

### 1. Comprueba tu entorno

```bash
lightspeed --check
```

Esto verifica que tu sistema operativo tenga las herramientas necesarias de filtrado de paquetes (nftables/iptables en Linux, pfctl en macOS, WinDivert en Windows).

### 2. Sondea tus relays

```bash
lightspeed --probe-proxies
```

Muestra la latencia de cada relay descubierto. El cliente selecciona automáticamente el más rápido en la primera ejecución; puedes cambiarlo eligiendo el que esté más cerca de tu **servidor de juego**, no de tu ubicación.

### 3. Arranca el interceptor

```bash
# Linux/macOS (requires root)
sudo lightspeed --start-interceptor --game rust --proxy YOUR_PROXY_IP:4434

# Windows (requires Administrator)
lightspeed --start-interceptor --game rust --proxy YOUR_PROXY_IP:4434
```

### 4. Lanza tu juego

Conéctate a cualquier servidor con normalidad. LightSpeed detecta el servidor de juego a partir de los paquetes salientes y empieza a tunelizar en pocos segundos.

### 5. Monitoriza

La CLI muestra estadísticas en vivo:
```
⚡ OPTIMIZING - 123.45.67.89:28015
Packets Sent: 142 | Packets Returned: 139 | Packets Delivered: 139
```

---

## Inicio rápido (GUI - Windows)

### 1. Descarga

Coge la última versión desde [Releases](https://github.com/ShibbityShwab/lightspeed/releases). Extrae todos los archivos y deja `WinDivert64.sys` y `WinDivert.dll` junto a `lightspeed-gui.exe`.

### 2. Ejecuta como Administrador

Haz clic derecho en `lightspeed-gui.exe` → **Ejecutar como administrador**. El interceptor necesita acceso a nivel de kernel (igual que el software de VPN).

### 3. Elige un relay y un juego

La GUI descubre los relays comunitarios y selecciona automáticamente el más rápido en la primera ejecución. Puedes cambiar el relay desde el desplegable y luego elegir tu juego.

### 4. Pulsa **⚡ OPTIMIZE MY ROUTE**

El estado cambia a "🎯 Finding your game server…"

### 5. Lanza tu juego

Conéctate a cualquier servidor. LightSpeed lo detecta en pocos segundos.

---

### GUI de macOS (sin probar)

La GUI compila para macOS pero no está **probada** en hardware real. La versión
publicada incluye un `tar.xz` pelado (cargo-dist 0.32 no admite `.app`/`.dmg`), así que para
generar un bundle en condiciones, ejecuta en un Mac:

```bash
cargo build --release -p lightspeed-gui
./tools/package-macos.sh 1.6.5
```

Esto crea `LightSpeed.app` y `LightSpeed-1.6.5.dmg`. La app está firmada
ad-hoc, así que el primer arranque requiere clic derecho → Abrir (o
`xattr -dr com.apple.quarantine LightSpeed.app`).

---

## Elegir el relay correcto

| Estás en | Servidor de juego en | Mejor región de relay |
|-----------|---------------|-------------------|
| Australia | US West | US West (Los Angeles) |
| Europa | US East | US East (New Jersey) |
| Sudeste asiático | Singapur | Singapur |
| Asia del Sur | India | Mumbai |
| Asia Oriental | Japón | Tokio |
| Sudamérica | US East | US East (New Jersey) |
| Donde sea | Tu misma región | El más cercano al servidor de juego |

> **Regla general:** elige el relay más cercano al **servidor de juego**, no el más cercano a ti. Tu tráfico va de tu PC → relay → servidor de juego, así que el tramo entre el relay y el servidor de juego es el que más importa.

---

## Corrección de errores en recepción (FEC)

La FEC añade un sobrecoste de ancho de banda de ~25% para recuperar paquetes perdidos sin retransmisión.

**Actívala cuando:**
- Tengas pérdida de paquetes (microtirones, rubber-banding)
- Estés en Wi-Fi con interferencias intermitentes

**Desactívala cuando:**
- Tu conexión ya esté saturada
- Tengas una conexión medida o con límite de datos
- Tu pérdida de paquetes sea < 0.1% (no aporta nada)

```bash
# CLI: enable FEC with default block size (K=4)
lightspeed --start-interceptor --game cs2 --proxy YOUR_PROXY:4434 --fec

# Custom block size (K=8 → 12.5% overhead)
lightspeed --start-interceptor --game cs2 --proxy YOUR_PROXY:4434 --fec --fec-k 8
```

---

## Avanzado: modo de servidor manual

Si la detección automática no funciona (puertos personalizados, juegos poco habituales):

```bash
# Redirect mode: game connects to localhost, LightSpeed forwards to real server
lightspeed --game rust --game-server 123.45.67.89:28015 --proxy YOUR_PROXY:4434
```

Luego configura tu juego para conectarse a `127.0.0.1:<port>` (el puerto local que imprime LightSpeed).

---

## Cambiar de servidor a mitad de sesión

LightSpeed detecta automáticamente cuándo te desconectas de un servidor y te conectas a otro. El estado muestra brevemente "🎯 Finding your game server…" y se fija en el nuevo destino. No hace falta hacer nada a mano.

---

## Bandeja del sistema (GUI de Windows)

- Haz clic en **×** para minimizar a la bandeja (no cierra la app)
- Haz doble clic en el icono del rayo para restaurarla
- Haz clic derecho para Conectar / Desconectar / Salir rápidamente

---

## Ver también

- [CLI Reference](CLI-REFERENCE.md) - todos los flags explicados
- [FAQ](faq.md) - preguntas frecuentes
- [Troubleshooting](troubleshooting.md) - cómo resolver problemas
- [Deploy Proxy](deploy-proxy.md) - ejecuta tu propio proxy
- [Supported Games](supported-games.md) - compatibilidad de juegos
