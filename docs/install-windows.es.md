# Instalar LightSpeed en Windows

Windows es la plataforma donde la GUI manda. El paquete `lightspeed-gui` es una app independiente que ya contiene el motor del cliente y el driver WinDivert, así que nunca necesitas descargar el cliente por separado.

---

## Qué descargar

Coge la última versión desde la [página de Releases](https://github.com/ShibbityShwab/lightspeed/releases/latest). Para Windows x86_64 (todos los PC Intel y AMD actuales), elige uno de estos:

| Archivo | Úsalo cuando |
|------|-------------|
| `lightspeed-gui-...-windows-msvc.msi` | Recomendado. Se instala en Program Files, añade un acceso directo en el menú Inicio y registra un desinstalador. |
| `lightspeed-gui-...-windows-msvc.zip` | Portátil. Descomprime donde quieras y ejecuta `lightspeed-gui.exe`. |

Ambos contienen la misma GUI más `WinDivert.dll` y `WinDivert64.sys`.

> **Windows en ARM64 todavía no es un objetivo publicado.** Las compilaciones de release apuntan a `x86_64-pc-windows-msvc`. En un dispositivo ARM64, la compilación x86_64 se ejecuta bajo la capa de emulación de Windows, pero no está probada en hardware ARM64 real.

### CLI para Windows (sin soporte)

También se publica una compilación de línea de comandos para Windows como zip (`lightspeed-client-...-windows-msvc.zip`). Se ofrece para scripting y uso sin interfaz, pero no tiene **soporte**: la GUI es la vía recomendada en Windows. Si la usas, ejecútala desde una terminal elevada (ver más abajo).

---

## Instalación (MSI)

1. Descarga el `.msi`.
2. Haz doble clic y sigue el asistente. Puede que Windows SmartScreen avise de un editor desconocido; la versión está atestiguada con Sigstore, así que puedes verificar la atestación si quieres.
3. Abre **LightSpeed** desde el menú Inicio.

## Instalación (zip portátil)

1. Descarga el `.zip`.
2. Haz clic derecho, elige **Extraer todo** y extrae a una carpeta donde puedas escribir (por ejemplo `C:\LightSpeed`). No lo ejecutes desde dentro del zip.
3. Ejecuta `lightspeed-gui.exe`.

---

## Privilegios de administrador

LightSpeed usa el driver WinDivert para interceptar el tráfico UDP del juego. WinDivert requiere derechos de **Administrador**.

- La GUI solicita elevación cuando necesita arrancar el interceptor. Acepta el aviso de UAC.
- Si usas la compilación de CLI, lánzala desde una terminal de **Administrador** (clic derecho en Windows Terminal o PowerShell y luego **Ejecutar como administrador**).

Sin elevación, el interceptor no puede engancharse y verás un error de interceptor en la vista de estado.

---

## Primera ejecución

1. Abre la GUI y acepta el aviso de UAC.
2. La GUI descubre los relays comunitarios automáticamente a través del registro firmado. No hace falta ninguna dirección de proxy.
3. Elige tu juego en la lista.
4. Arranca el interceptor, lanza tu juego y conéctate a un servidor.

La GUI muestra el estado del relay, el estado del registro y los contadores de paquetes para que veas el tráfico fluyendo.

---

## Comprueba que funciona

**Comprueba el descubrimiento de relays y el registro.** Abre la vista de estado de la GUI. Deberías ver los relays comunitarios listados (Los Angeles, New Jersey, Singapore, Frankfurt, Tokyo, Mumbai, Madrid, Sydney) con estado saludable, y una línea de registro que indique que el handshake QUIC/auth se completó. Si usas la compilación de CLI, ejecuta:

```powershell
lightspeed-client.exe --probe-proxies
```

Esto hace una pasada de descubrimiento/sondeo e imprime un informe visible con cada relay descubierto y su latencia. Deberías ver los ocho relays comunitarios.

**Comprueba el registro en el plano de control.** Con la compilación de CLI:

```powershell
lightspeed-client.exe --test-control
```

Esto se conecta al plano de control QUIC, registra una sesión, hace ping y se desconecta, imprimiendo el resultado de cada paso. Un registro correcto demuestra que el plano de control es accesible y que la autenticación funciona.

**Comprueba el flujo de paquetes.** En la GUI, los contadores de paquetes deberían subir cuando tu juego esté conectado a un servidor. Si "Packets Sent" se queda en 0, el interceptor todavía no ha visto tráfico del juego; consulta [Troubleshooting](troubleshooting.md).

---

## Registros

La GUI escribe un log de trazas en:

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

Pega esta ruta en la barra de direcciones del Explorador de archivos para abrirlo. Adjúntalo a un informe de error si algo va mal. Consulta [Troubleshooting](troubleshooting.md) para saber qué significa cada línea del log.

---

## Desinstalación

- **MSI:** Configuración → Aplicaciones → Aplicaciones instaladas → LightSpeed → Desinstalar.
- **Zip:** borra la carpeta extraída. El archivo de log en `%LOCALAPPDATA%\Lightspeed\` se queda ahí; bórralo a mano si quieres una eliminación limpia.

---

## Siguientes pasos

- [Supported Games](supported-games.md)
- [Troubleshooting](troubleshooting.md)
- [CLI Reference](CLI-REFERENCE.md)
