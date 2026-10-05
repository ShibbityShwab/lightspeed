# Instalar LightSpeed en macOS

> [!WARNING]
> Traducción asistida por máquina, no revisada por un hablante nativo. La [versión en inglés](install-macos.md) es la autoritativa.

macOS es una plataforma donde la CLI manda. La GUI compila para macOS pero no está probada en hardware real, así que el cliente de línea de comandos (`lightspeed-client`) es la vía con soporte.

---

## Qué descargar

Coge la última versión desde la [página de Releases](https://github.com/ShibbityShwab/lightspeed/releases/latest). Elige el archivo que corresponda a la CPU de tu Mac:

| Tu Mac | Target | Archivo |
|----------|--------|------|
| Apple Silicon (M1 y posteriores) | `aarch64-apple-darwin` | `lightspeed-client-...-aarch64-apple-darwin.tar.xz` |
| Intel | `x86_64-apple-darwin` | `lightspeed-client-...-x86_64-apple-darwin.tar.xz` |

¿No sabes cuál tienes? Ejecuta:

```bash
uname -m
```

`arm64` significa Apple Silicon; `x86_64` significa Intel.

---

## Instalación

El instalador de shell es la vía más fácil. Detecta tu arquitectura e instala el cliente:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ShibbityShwab/lightspeed/releases/latest/download/lightspeed-client-installer.sh | sh
```

O instala a mano desde el archivo:

```bash
# Replace the filename with the one you downloaded.
tar -xf lightspeed-client-...-aarch64-apple-darwin.tar.xz
sudo mv lightspeed-client /usr/local/bin/
```

Comprueba que el binario arranca:

```bash
lightspeed-client --version
```

---

## Privilegios de root

El interceptor de macOS usa `pfctl` (el filtro de paquetes integrado) para redirigir el tráfico UDP del juego. `pfctl` requiere root, así que ejecuta el cliente con `sudo` cuando arranques el interceptor:

```bash
sudo lightspeed-client --start-interceptor --game rust
```

El descubrimiento, el sondeo y los diagnósticos (`--probe-proxies`, `--test-control`, `--check`) no necesitan root.

---

## Primera ejecución

El cliente descubre los relays comunitarios automáticamente a través del registro firmado. No hace falta ninguna dirección de proxy.

```bash
# Probe the community relays and print a report
lightspeed-client --probe-proxies

# Start the interceptor for your game
sudo lightspeed-client --start-interceptor --game rust
```

---

## Comprueba que funciona

**Comprueba el descubrimiento de relays.** Ejecuta:

```bash
lightspeed-client --probe-proxies
```

Esto hace una pasada de descubrimiento/sondeo e imprime un informe visible con cada relay descubierto y su latencia. Deberías ver los ocho relays comunitarios (Los Angeles, New Jersey, Singapore, Frankfurt, Tokyo, Mumbai, Madrid, Sydney).

**Comprueba el registro en el plano de control.** Ejecuta:

```bash
lightspeed-client --test-control
```

Esto se conecta al plano de control QUIC, registra una sesión, hace ping y se desconecta, imprimiendo el resultado de cada paso. Un registro correcto demuestra que el plano de control es accesible y que la autenticación funciona.

**Comprueba el entorno.** Ejecuta:

```bash
lightspeed-client --check
```

Esto informa de la disponibilidad del interceptor, el estado de root, la detección de juegos y la accesibilidad del proxy.

**Comprueba el flujo de paquetes.** Cuando el interceptor esté en marcha y tu juego conectado a un servidor, los contadores de paquetes del cliente deberían subir. Si "Packets Sent" se queda en 0, el interceptor todavía no ha visto tráfico del juego; consulta [Troubleshooting](troubleshooting.md).

---

## Nota sobre Gatekeeper

Si macOS bloquea el binario con "cannot be opened because the developer cannot be verified", elimina el atributo de cuarentena:

```bash
xattr -d com.apple.quarantine /usr/local/bin/lightspeed-client
```

Hazlo solo si descargaste el binario desde la página oficial de Releases.

---

## Siguientes pasos

- [Supported Games](supported-games.md)
- [Troubleshooting](troubleshooting.md)
- [CLI Reference](CLI-REFERENCE.md)
