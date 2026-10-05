# Resolución de problemas

---

## Diagnóstico rápido

Ejecuta primero la comprobación de entorno integrada: detecta la mayoría de los problemas:

```bash
lightspeed --check
```

Esto verifica: disponibilidad del interceptor, herramientas de filtrado de paquetes, resolución del perfil de juego y conectividad con el proxy.

---

## Problemas comunes

### "Interceptor not available"

**CLI:** tu sistema operativo no tiene las herramientas de filtrado de paquetes necesarias o te faltan privilegios.

| OS | Requerido | Cómo arreglarlo |
|----|----------|------------|
| Linux | nftables o iptables + root | `sudo lightspeed ...` |
| macOS | pfctl (integrado) + root | `sudo lightspeed ...` |
| Windows | driver WinDivert + Administrador | Clic derecho → Ejecutar como Administrador |

Verifícalo con:
```bash
lightspeed --check
```

### "No game traffic seen" / Packets Sent se queda en 0

**CLI:** el interceptor no encuentra paquetes de juego en el rango de puertos esperado.

1. Asegúrate de que tu juego esté **conectado a un servidor** (no solo en el menú principal o el lobby)
2. Verifica el juego con `--scan-processes`:
   ```bash
   lightspeed --scan-processes
   ```
3. Prueba otro perfil de juego o usa el modo de servidor manual

**GUI (Windows):** espera 15 segundos. Si aparece el aviso ámbar "⚠ No game traffic seen":
1. Abre PowerShell como administrador:
   ```powershell
   Get-NetUDPEndpoint -OwningProcess (Get-Process RustClient).Id |
     Where-Object LocalPort -gt 1024 |
     Select-Object LocalPort
   ```
2. Usa el puerto que aparezca en **Advanced → set server manually**

### "🎯 Finding your game server…" nunca se resuelve

El detector no ha visto 3 paquetes hacia el mismo destino en 1.5 segundos.

1. Asegúrate de estar conectado a un servidor de juego (mueve el personaje para generar tráfico)
2. Si los paquetes siguen sin detectarse después de 15 segundos, tu servidor está en un puerto no estándar: usa el modo de servidor manual
3. Para y reinicia el interceptor después de conectarte al servidor

### Packets Sent sube, Packets Delivered = 0

Los paquetes llegan al proxy pero las respuestas no llegan a tu juego. Normalmente es un problema de firewall.

**Linux:**
```bash
sudo iptables -I INPUT -p udp --sport 4434 -j ACCEPT
```

**macOS:**
```bash
sudo pfctl -d  # Temporarily disable pf to test
```

**Windows:**
```powershell
# Check if the firewall rule exists
netsh advfirewall firewall show rule name="LightSpeed WinDivert Tunnel"

# Add it manually if missing
netsh advfirewall firewall add rule name="LightSpeed" protocol=UDP dir=in action=allow program="C:\path\to\lightspeed-gui.exe"
```

### La comprobación de estado del proxy falla

```bash
# Test connectivity
curl http://YOUR_PROXY_IP:8080/health

# Expected response:
# {"status":"ok","node_id":"relay-1","uptime_secs":86400,...}
```

Si no es accesible:
- Comprueba que el proxy esté en marcha: `systemctl status lightspeed-proxy`
- Comprueba que el firewall permita UDP 4434 y TCP 8080
- Comprueba los logs del proxy: `journalctl -u lightspeed-proxy --tail 50`

### El juego se desconecta cuando arranca el interceptor

El interceptor se apodera de los paquetes antes de que el juego pueda recibir respuestas, y la ruta de inyección falla.

**Windows:**
1. Verifica que `WinDivert64.sys` y `WinDivert.dll` estén junto al `.exe`
2. Desconecta los adaptadores de red secundarios (adaptadores virtuales de Docker, VMware, Hamachi)
3. Conéctate al servidor de juego **antes** de arrancar el interceptor

**Linux:**
1. Revisa las reglas de nftables: `sudo nft list ruleset | grep lightspeed`
2. Si las reglas están obsoletas: `sudo lightspeed --check` para diagnosticar

### "WinDivert open failed" / `FWP_E_IN_USE` (0x8032000A) en Windows

WinDivert registra un callout/filtro WFP por cada handle abierto. Si un handle nunca se cierra, ese estado del filtro se queda ahí, y el siguiente `WinDivertOpen` falla con `FWP_E_IN_USE` (0x8032000A) aunque ningún proceso esté usando WinDivert a la vista.

Las compilaciones recientes de LightSpeed cierran tanto el handle de captura como el de inyección en todas las rutas de apagado ordenado, incluido Ctrl+C en `--watch` y en `--start-interceptor`, y **Quit** en la GUI, y el bucle de recepción se desbloquea con `WinDivertShutdown` antes del cierre para que el desmontaje sea determinista. Un kill forzado (`taskkill /f`, un crash o cerrar la ventana de la consola) todavía puede dejar al driver WinDivert 2.2.x con estado obsoleto; es una limitación del driver de aguas arriba (basil00/WinDivert#294, #406) que el espacio de usuario no puede limpiar una vez que el proceso ha desaparecido.

Si aun así te topas con ello:

1. **Cierra con orden y espera un momento**: usa Ctrl+C en la CLI o **Quit** en la GUI, y luego deja uno o dos segundos para que se cierren los handles antes de relanzar.
2. **Para el servicio WinDivert** (evita un reinicio en algunos casos; ten en cuenta que el driver se comparte con cualquier otra app basada en WinDivert de la máquina):
   ```powershell
   sc stop windivert
   ```
3. **Apagado completo, no reinicio**: el "Reiniciar" de Windows puede reutilizar la sesión del kernel que guarda el estado obsoleto; un **Apagar → encender** completo lo limpia.

> **Consejo:** en la v1.2.2 y anteriores, un bug aparte (la autenticación del plano de datos rechazaba todos los paquetes, issue #59) congelaba la conexión y obligaba a los usuarios a matar el cliente una y otra vez, que es lo que provocó la mayoría de los informes de `FWP_E_IN_USE`. Ese bug de autenticación está corregido en la v1.2.3.

---

## Problemas de la GUI de Windows

Estos aplican a la app `lightspeed-gui` en Windows.

### Quit no hizo nada y dejó un proceso zombi

En versiones anteriores a la v1.4.2, elegir **Quit** en el menú de la bandeja podía dejar el proceso corriendo en segundo plano (un zombi), así que la ventana se cerraba pero el motor seguía funcionando y un arranque posterior se comportaba de forma extraña. La v1.4.2 lo arregla: Quit ahora termina el proceso de forma limpia.

Si estás en una compilación antigua y el proceso se ha quedado colgado, termínalo a mano:

```powershell
Get-Process lightspeed-gui -ErrorAction SilentlyContinue | Stop-Process
```

Luego actualiza a la v1.4.2 o posterior.

### Se abre una segunda instancia en lugar de enfocar la primera

En versiones anteriores a la v1.4.2, lanzar la GUI dos veces apilaba una segunda ventana y un segundo motor. La GUI ahora mantiene un guardián de instancia única: un segundo lanzamiento muestra un aviso breve "LightSpeed is already running" y sale en lugar de arrancar otro motor. Para forzar una segunda instancia de todos modos (para diagnósticos), pasa `--force` o define `LIGHTSPEED_GUI_FORCE=1`.

### No se descubre ningún relay

La GUI descubre los relays comunitarios a través del registro firmado. Si la lista de relays se queda vacía:

1. Confirma que tienes acceso a internet y que ningún firewall ni VPN está bloqueando el HTTPS saliente hacia el registro.
2. Revisa el log de la GUI (ver más abajo) por si hay un error al obtener el registro o al verificar la firma.
3. En la compilación de CLI, ejecuta `lightspeed-client --probe-proxies` para ver el informe de descubrimiento y sondeo directamente. Si la CLI tampoco encuentra nada, el problema es de red, no de la GUI.
4. Reinicia la GUI después de arreglar la conectividad; el descubrimiento se ejecuta al arrancar.

### Cómo encontrar y abrir el log de la GUI

La GUI escribe su log de trazas en:

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

Pega esa ruta en la barra de direcciones del Explorador de archivos para abrir la carpeta, y luego abre `gui-trace.log` en cualquier editor de texto. Adjúntalo a un informe de error.

### "Heartbeat 0 in" en el log

Una línea como `Heartbeat 0 in` significa que el motor ha enviado cero heartbeats de keepalive en la ventana actual. En la práctica aparece cuando el cliente todavía no ha establecido una conexión funcional con el plano de control, así que no ha salido ningún heartbeat. Causas habituales:

- El cliente todavía no se ha registrado con un relay (revisa la línea de registro en la vista de estado).
- El relay seleccionado no es accesible.
- El interceptor no ha arrancado, así que no hay ninguna sesión activa.

Cuando el registro tiene éxito y los heartbeats empiezan a fluir, el contador sube. Si se queda en 0 mientras un relay aparece como saludable, ejecuta `lightspeed-client --test-control` para aislar si el plano de control es accesible.

---

## Logs para informes de error

Ejecuta con logging de depuración para capturar diagnósticos detallados:

```bash
# CLI
RUST_LOG=debug lightspeed --start-interceptor --game rust --proxy YOUR_PROXY:4434 2>&1 | tee lightspeed.log

# Windows GUI
cd C:\path\to\lightspeed
lightspeed-gui.exe 2>&1 | tee lightspeed-log.txt
```

Adjunta el archivo de log a tu [issue de GitHub](https://github.com/ShibbityShwab/lightspeed/issues).

---

## ¿Sigues atascado?

- [FAQ](faq.md) - preguntas frecuentes
- [GitHub Issues](https://github.com/ShibbityShwab/lightspeed/issues) - busca informes existentes
- Abre un issue nuevo con tu sistema operativo, tu juego y la salida del log
