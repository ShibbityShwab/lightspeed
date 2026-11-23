# Solución de problemas

> [!WARNING]
> Traducción asistida por máquina, no revisada por un hablante nativo. La [versión en inglés](troubleshooting.md) es la autoritativa.

---

## Diagnóstico rápido

Primero ejecute la verificación del entorno integrado; detecta la mayoría de los problemas:

```bash
lightspeed --check
```

Esto verifica: la disponibilidad del interceptor, las herramientas de filtrado de paquetes, la resolución del perfil del juego y la conectividad del proxy.

---

## Problemas comunes

### "Interceptor no disponible"

**CLI:** Su sistema operativo no tiene las herramientas de filtrado de paquetes necesarias o carece de privilegios.

| SO | Requerido | Cómo solucionarlo |
|----|----------|------------|
| Linux | nftables o iptables + raíz | `sudo lightspeed ...` |
| MacOS | pfctl (integrado) + raíz | `sudo lightspeed ...` |
| Ventanas | Controlador WinDivert + Administrador | Haga clic derecho → Ejecutar como administrador |

Verificar con:
```bash
lightspeed --check
```

### "No se ve tráfico del juego" / Los paquetes enviados permanecen en 0

**CLI:** El interceptor no puede encontrar paquetes de juegos en el rango de puertos esperado.

1. Asegúrate de que tu juego esté **conectado a un servidor** (no solo al menú principal o al lobby)
2. Verificar el juego con `--scan-processes`:
   ```bash
   lightspeed --scan-processes
   ```
3. Pruebe con un perfil de juego diferente o use el modo de servidor manual

**GUI (Windows):** Espere 15 segundos. Si aparece el cartel ámbar "⚠ No se ve tráfico en el juego":
1. Abra PowerShell elevado:
   ```powershell
   Get-NetUDPEndpoint -OwningProcess (Get-Process RustClient).Id |
     Where-Object LocalPort -gt 1024 |
     Select-Object LocalPort
   ```
2. Utilice el puerto que se muestra en **Avanzado → configurar el servidor manualmente**

### "🎯 Encontrar tu servidor de juegos..." nunca se resuelve

El detector no ha detectado 3 paquetes con el mismo destino en 1,5 segundos.

1. Asegúrate de estar conectado a un servidor de juego (mueve tu personaje para generar tráfico)
2. Si los paquetes aún no se detectan después de 15 segundos, su servidor está en un puerto no estándar; use el modo de servidor manual
3. Detenga y reinicie el interceptor después de conectarse al servidor.

### Paquetes enviados subiendo, Paquetes entregados = 0

Los paquetes llegan al proxy pero las respuestas no llegan a tu juego. Generalmente es un problema de firewall.

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
# Check if the stable rule exists (one rule, not one per boost)
netsh advfirewall firewall show rule name="LightSpeed WinDivert Tunnel"

# Add it manually if missing
netsh advfirewall firewall add rule name="LightSpeed WinDivert Tunnel" protocol=UDP dir=in action=allow program="C:\path\to\lightspeed-gui.exe"
```

Las reglas de compilaciones anteriores se denominaron `LightSpeed WinDivert Tunnel <port>`; actual
Las compilaciones eliminan la del puerto que utilizan y mantienen una única regla estable. Cualquiera
Las reglas restantes por puerto se pueden eliminar de forma segura:
`netsh advfirewall firewall delete rule name="LightSpeed WinDivert Tunnel 4434"`
(coincide con el puerto mostrado por `netsh advfirewall firewall show rule name=all`).

### `sendmsg error: ... 10040` / `WSAEMSGSIZE` (Windows) o `EMSGSIZE` (Linux/macOS)

El datagrama que el cliente intentó enviar es mayor que la MTU de ruta **real**. el
La abrazadera de túnel predeterminada es de 1492 bytes (seguro para PPPoE), pero una VPN, un ajuste de DPI como
Zapret o un adaptador de MTU reducido pueden ser más pequeños, por lo que un paquete que se ajuste a la abrazadera
todavía es rechazado por el sistema operativo (`Windows error 10040`).

Solución: baje el límite de MTU de ruta del túnel, en `config.toml`:

```toml
[tunnel]
# 1280-1500. Start at 1400, then 1350 if the error persists.
mtu = 1400
```

o pase `--mtu 1400` en la CLI. Las construcciones actuales también reducen el presupuesto.
automáticamente cuando se rechaza un envío, vuelva a intentar ese datagrama una vez con
fragmentación permitida y registre el error del sistema operativo en el estado en lugar de
informando que el relé está en silencio.

Solución alternativa inmediata mientras el cliente no se modifica: baje la velocidad del adaptador de Windows
MTU para que el juego emita paquetes más pequeños, luego reinicia el juego:

```powershell
netsh interface ipv4 show subinterfaces
netsh interface ipv4 set subinterface "Ethernet" mtu=1400 store=persistent
```

### La comprobación del estado del proxy falla

```bash
# Test connectivity
curl http://YOUR_PROXY_IP:8080/health

# Expected response:
# {"status":"ok","node_id":"relay-1","uptime_secs":86400,...}
```

Si es inalcanzable:
- Verifique que el proxy se esté ejecutando: `systemctl status lightspeed-proxy`
- Verifique que el firewall permita UDP 4434 y TCP 8080
- Verifique los registros de proxy: `journalctl -u lightspeed-proxy --tail 50`

### El plano de control QUIC falla con `error 120` / `no_application_protocol`

El cliente no pudo completar el protocolo de enlace del plano de control QUIC con un relé. cuando el
El reintento de oferta vacía de una sola vez también falló, el registro termina con este veredicto (citado
palabra por palabra del cliente; `{err}` es el error QUIC subyacente y `{addr}` el
dirección del respondedor):

```text
{err}: the responder at {addr} closed the QUIC handshake with TLS alert 120 (no_application_protocol), and the empty-offer retry also failed. The client cannot tell these two causes apart from the handshake alone: (1) a QUIC-intercepting middlebox on this path (VPN/AV/ISP) that does not carry the ALPN extension (see #140); or (2) a LightSpeed relay on a different ALPN revision -- version skew, where the relay requires an ALPN name this client does not offer.
```

**Lo que el cliente ya hizo.** Los clientes actuales siempre ofrecen el nombre ALPN versionado.
`lightspeed-control/1`. Si el relé responde con la alerta TLS 120, el cliente vuelve a intentar
**una vez** con una oferta ALPN vacía y luego recuerda la respuesta de ese relevo para el resto
del proceso, por lo que en la operación normal del cliente el apretón de manos adicional condenado ocurre en
más una vez por relevo por lanzamiento. Un retroceso exitoso es normal contra un relé que
no se ha actualizado (el relé predeterminado es `[network] control_alpn = "off"`): el
La línea `QUIC connection established` muestra `alpn=none` y se continúa con el registro.

**Qué significa el error final.** El reintento de la oferta vacía también falló, por lo que hay
exactamente dos posibilidades y el apretón de manos por sí solo no puede diferenciarlas:

1. **Un middlebox que intercepta QUIC en tu camino**: una VPN, antivirus/firewall con
   Inspección HTTPS/QUIC o equipo ISP que no lleva la extensión ALPN
   ([#140](https://github.com/ShibbityShwab/lightspeed/issues/140)). Si cada relevo
   falla en una red pero los mismos relés funcionan en otra, el camino es el común
   factor.
2. **Versión sesgada**: el relé requiere una revisión ALPN que este cliente no ofrece
   (por ejemplo, un relé configurado en `control_alpn = "required"` y ejecutando un protocolo
   revisión más reciente que este cliente). Esto falla ruidosamente por diseño, nunca como un silencio.
   degradación: un cliente actual contra un relé en `lightspeed-control/1` negocia,
   y un relevo que requiere algo que este cliente no ofrece lo rechaza con 120.

**Cómo reducirlo:**

- Ejecute `lightspeed-client --test-control --proxy <relay-ip>` para ver en qué etapa
  El relé está en. El valor de `alpn=` en la línea conectada y en el resumen final es el
  Protocolo de aplicación negociado: `lightspeed-control/1` significa que el relé aceptó el
  oferta versionada, `none` significa que aceptó una oferta vacía después del respaldo (una versión anterior
  relevo), y una carrera que falla con el veredicto de esta sección es el caso anterior.
- Si todos los relés fallan en esta red pero los mismos relés funcionan en otros lugares, sospeche del
  middlebox: deshabilite la VPN o la inspección HTTPS/QUIC del producto de seguridad y vuelva a intentarlo,
  luego adjunte una captura UDP 4433 al n.° 140.
- Si solo falla un relé, compare el modo ALPN de ese relé con la compilación de su cliente: el
  La línea de inicio del relé y el campo `control_alpn` en su `GET /health` informan el
  modo configurado (consulte [Referencia CLI](CLI-REFERENCE.md#proxy-configuration)). si tu
  operar ese relé, `control_alpn = "off"` restaura el comportamiento anterior para versiones anteriores
  clientes; El procedimiento del operador y la regla de pedido del cliente primero están en
  [Implementar proxy](deploy-proxy.md) y el [plan de implementación de ALPN](quic-alpn-rollout.md).

### El juego se desconecta cuando se inicia el interceptor

El interceptor captura paquetes antes de que el juego pueda recibir respuestas y la ruta de inyección falla.

**Windows:**
1. Verifique que `WinDivert64.sys` y `WinDivert.dll` estén al lado del `.exe`.
2. Desconecte los adaptadores de red secundarios (adaptadores virtuales Docker, VMware, Hamachi)
3. Conéctese al servidor del juego **antes** de iniciar el interceptor

**Linux:**
1. Verifique las reglas de nftables: `sudo nft list ruleset | grep lightspeed`
2. Si las reglas están obsoletas: `sudo lightspeed --check` para diagnosticar

### "Error al abrir WinDivert" / `FWP_E_IN_USE` (0x8032000A) en Windows

Solo los handles que llevan un filtro de captura real —el de interceptación (`udp and outbound …`) y el de sombra (`udp and inbound …`)— poseen un callout/filtro WFP. El handle de anclaje (pin) de toda la vida del proceso y el handle de inyección usan el filtro `"false"`, no desvían tráfico y no poseen ningún objeto WFP. Por tanto, un callout/filtro WFP no es «uno por cada handle abierto», y un pin retenido no es un bloqueo del estado WFP: en la traza de campo del 23-11-2026 el pin está abierto y la siguiente apertura de interceptación sigue fallando con `FWP_E_IN_USE`.

`FWP_E_IN_USE` significa que el objeto WFP todavía está referenciado (por ejemplo, un filtro de un proceso anterior que el kernel no ha terminado de eliminar tras un cierre forzado), o que un callout se está registrando/deregistrando de forma concurrente. «Ningún proceso usa WinDivert de forma visible» no demuestra que nada mantenga la referencia, y hoy no se captura el estado WFP, así que no trate el texto del error como una causa raíz.

LightSpeed cierra los handles de interceptación, inyección y sombra en todas las rutas de apagado ordenado (Ctrl+C en `--watch`/`--start-interceptor`, **Salir** en la GUI), desbloquea el bucle de recepción con `WinDivertShutdown` antes del cierre y espera la confirmación de los hilos propietarios. Ese determinismo se aplica a la **propiedad de los handles** de la app; el teardown WFP del kernel es una ruta distinta y sus fallos no son visibles desde el espacio de usuario.

Si sigue ocurriendo:

1. **Reintente Iniciar una vez.** El próximo parche reintenta `FWP_E_IN_USE` automáticamente mientras el pin está retenido (acotado, ~5 s) y puede reciclar el pin una vez; 1.12.5 solo reintenta mientras **no** hay pin retenido, así que ahí reintentar significa Detener y Optimizar de nuevo.
2. **Salga de forma ordenada y espere un momento**: use Ctrl+C en la CLI o **Salir** en la GUI, y dé a los handles uno o dos segundos para cerrarse antes de relanzar.
3. **Detenga el servicio WinDivert** (administrador; es un reinicio explícito que LightSpeed no hace automáticamente y afecta a cualquier otra app basada en WinDivert del equipo: compruebe antes que no haya otro cliente WinDivert en ejecución):
   ```powershell
   sc stop windivert
   ```
4. **Apagar por completo** es el último recurso, y solo cuando no se puede descartar a un usuario externo de WinDivert: un **Reiniciar** de Windows puede reutilizar la sesión del kernel que retiene el estado obsoleto, así que un **Apagar → encender** completo es la única limpieza fiable a nivel de máquina. No es la primera solución y LightSpeed no le pedirá reiniciar sin ese motivo.

WinDivert 2.2 mantiene su driver cargado tras cerrar el último handle: el proveedor WFP y sus 20 subcapas permanecen (1 / 20) y `WinDivert64.sys` sigue bloqueado hasta que se detiene el servicio o se reinicia el equipo. El pin no cambia eso y no es una garantía de que el driver se descargue al Detener.

> **Consejo:** en v1.2.2 y anteriores, un fallo aparte (la autenticación del plano de datos rechazaba todos los paquetes, issue #59) congelaba la conexión y obligaba a matar el cliente repetidamente, lo que provocó la mayoría de los informes de `FWP_E_IN_USE`. Ese fallo de autenticación está corregido en v1.2.3.

---

## Problemas con la GUI de Windows

Estos se aplican a la aplicación `lightspeed-gui` en Windows.

### Salir no hizo nada y dejó un proceso zombie

En versiones anteriores a la v1.4.2, elegir **Salir** en el menú de la bandeja podía dejar el proceso ejecutándose en segundo plano (un zombie), por lo que la ventana se cerraba pero el motor seguía funcionando y un inicio posterior se comportaba de manera extraña. v1.4.2 soluciona este problema: Salir ahora finaliza el proceso limpiamente.

Si tiene una versión anterior y el proceso se bloquea, finalícelo manualmente:

```powershell
Get-Process lightspeed-gui -ErrorAction SilentlyContinue | Stop-Process
```

Luego actualice a v1.4.2 o posterior.

### Se abre una segunda instancia en lugar de centrarse en la primera.

En versiones anteriores a la v1.4.2, al iniciar la GUI dos veces se apilaba una segunda ventana y un segundo motor. La GUI ahora tiene una guardia de instancia única: un segundo lanzamiento muestra un breve aviso de "LightSpeed ​​ya está funcionando" y sale en lugar de arrancar otro motor. Para forzar una segunda instancia de todos modos (para diagnóstico), pase `--force` o configure `LIGHTSPEED_GUI_FORCE=1`.

### No se descubrieron relés

La GUI descubre los retransmisiones de la comunidad a través del registro firmado. Si la lista de retransmisiones permanece vacía:

1. Confirme que tiene acceso a Internet y que un firewall o VPN no está bloqueando el HTTPS saliente al registro.
2. Verifique el registro de la GUI (ver a continuación) para ver si hay un error de búsqueda del registro o de verificación de firma.
3. En la compilación de CLI, ejecute `lightspeed-client --probe-proxies` para ver el informe de descubrimiento y sondeo directamente. Si la CLI tampoco encuentra nada, el problema está en el lado de la red, no en la GUI.
4. Reinicie la GUI después de arreglar la conectividad; el descubrimiento se ejecuta al inicio.

### Cómo encontrar y abrir el registro de la GUI

La GUI escribe su registro de seguimiento en:

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

Pegue esa ruta en la barra de direcciones del Explorador de archivos para abrir la carpeta, luego abra `gui-trace.log` en cualquier editor de texto. Adjúntelo a un informe de error.

### "Latido 0 en" en el registro

Una línea como `Heartbeat 0 in` significa que el motor no ha enviado latidos de mantenimiento de actividad en la ventana actual. En la práctica, aparece cuando el cliente aún no ha establecido una conexión de plano de control que funcione, por lo que no se han producido latidos. Causas comunes:

- El cliente aún no se ha registrado con un relé (verifique la línea de registro en la vista de estado).
- El relé seleccionado es inalcanzable.
- El interceptor no se ha iniciado, por lo que no hay ninguna sesión activa.

Una vez que el registro se realiza correctamente y los latidos del corazón comienzan a fluir, el contador sube. Si permanece en 0 mientras un relé se muestra en buen estado, ejecute `lightspeed-client --test-control` para aislar si se puede acceder al plano de control.

---

## Registros para informes de errores

Ejecute con el registro de depuración para capturar diagnósticos detallados:

```bash
# CLI
RUST_LOG=debug lightspeed --start-interceptor --game rust --proxy YOUR_PROXY:4434 2>&1 | tee lightspeed.log

# Windows GUI
cd C:\path\to\lightspeed
lightspeed-gui.exe 2>&1 | tee lightspeed-log.txt
```

Adjunte el archivo de registro a su [problema de GitHub] (https://github.com/ShibbityShwab/lightspeed/issues).

---

## ¿Sigues atascado?

- [Preguntas frecuentes](faq.md) - preguntas comunes
- [Problemas de GitHub](https://github.com/ShibbityShwab/lightspeed/issues) - busque informes existentes
- Abra una nueva edición con su sistema operativo, juego y salida de registro.