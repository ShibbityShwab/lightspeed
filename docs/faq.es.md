# Preguntas frecuentes

---

## Conceptos básicos

### ¿LightSpeed es realmente gratis?

Sí. LightSpeed es gratis para uso personal y no comercial bajo la Licencia de Software de LightSpeed. El uso comercial requiere una licencia de pago; consulta [LICENSE](../LICENSE). Tú ejecutas tu propio proxy en un VPS pequeño (consulta la [guía de despliegue](../infra/README.md)). No hay suscripciones, ni tarifas por uso, ni planes de pago.

### ¿LightSpeed puede provocar que me baneen?

No. LightSpeed usa la misma clase de driver de red a nivel de sistema operativo (WinDivert/nftables/pfctl) que otras herramientas de captura de paquetes. No modifica archivos, memoria ni procesos del juego. Todos los sistemas antitrampas importantes (EAC, VAC, BattlEye, Riot Vanguard) lo permiten. Los servidores de juego ven tu dirección IP real: esto es un túnel transparente, no una VPN ni un anonimizador.

### ¿Por qué el interceptor necesita root/Administrador?

La interceptación de paquetes a nivel de kernel requiere privilegios elevados, por la misma razón por la que las VPN y los firewalls los necesitan. En Linux usa nftables/iptables. En macOS usa pfctl. En Windows usa WinDivert (un driver de kernel firmado). Sin root, todavía puedes usar el modo redirect (`--game-server`).

### ¿Qué plataformas tienen soporte?

| Plataforma | Interceptor | Modo Redirect | GUI |
|----------|-------------|---------------|-----|
| Windows 10/11 | ✅ WinDivert | ✅ | ✅ egui |
| Linux | ✅ nftables/iptables | ✅ | ❌ solo CLI |
| macOS | ✅ pfctl | ✅ | ❌ solo CLI |
| Linux ARM64 | ✅ | ✅ | ❌ |

---

## Cómo funciona

### ¿Cómo reduce el ping LightSpeed en la práctica?

LightSpeed **no** hace que tu tráfico vaya más rápido: los paquetes no pueden superar la velocidad de la luz. Lo que hace es *enrutar de forma proactiva* tu tráfico por el camino más rápido disponible, evitando la congestión y los desvíos innecesariamente largos.

Tu ISP envía los paquetes por el camino que le resulta más barato a *él*, a menudo congestionado o con rodeos. LightSpeed manda tus paquetes a través de un proxy en un centro de datos importante con conexiones directas de backbone hacia las regiones de los servidores de juego. Si ese camino es más corto o está menos congestionado que la ruta por defecto de tu ISP, tu ping baja y se estabiliza. La mejora típica: 10-40ms.

### Mi ping ha SUBIDO. ¿Por qué?

Dos motivos habituales:

1. **Ubicación equivocada del proxy**: si el proxy está más lejos del servidor de juego que tu camino directo, el salto extra añade latencia. Es la causa más común. Regla general: elige el proxy más cercano al **servidor de juego**, no el más cercano a ti.
2. **Proxy mal conectado**: no todos los centros de datos son iguales. Un proxy solo ayuda si ese centro de datos está cerca de un backbone importante de internet o de un punto de intercambio. Un VPS barato en la ciudad "correcta" pero con un enlace ascendente congestionado o residencial puede ser más lento que tu ruta directa.

LightSpeed solo puede optimizar la ruta que le das. Si lo apuntas a un proxy mal situado, tu ping subirá: ese es el comportamiento esperado, no un bug.

### ¿Qué proxy debería elegir?

El más cercano a la **región del servidor de juego**. Ejemplos:
- Juegas en servidores de US West → elige un proxy de US West
- Juegas en servidores de Singapur desde Australia → elige un proxy de Singapur
- Juegas en servidores de la UE desde Norteamérica → elige un proxy de Frankfurt/Londres

### Una nota sobre la realidad del enrutamiento (BGP)

El enrutamiento real de internet lo gobierna **BGP** (Border Gateway Protocol): los contratos y políticas que usan los ISP y los proveedores de tránsito para pasarse el tráfico. Tus paquetes no viajan en línea recta; siguen el camino que decidan las tablas BGP y los acuerdos de peering, y los proveedores priorizan o despriorizan ciertas rutas de forma rutinaria por motivos de coste o de política.

Qué significa eso para ti:

- Un proxy solo ayuda si se encuentra en una ruta BGP *mejor* que la ruta por defecto de tu conexión doméstica, normalmente un centro de datos cerca de un backbone importante o de un punto de peering.
- "Más cerca en el mapa" no siempre significa "más rápido en el cable".
- Las herramientas de optimización de rutas (incluida LightSpeed) estiman y reenrutan, pero el camino físico lo dictan en última instancia las redes intermedias, que ni tú ni LightSpeed controláis.

### ¿Qué tan rápida es la detección automática?

Normalmente 1-3 segundos después de conectarte a un servidor de juego. El interceptor espera a ver 3 paquetes hacia el mismo destino en 1.5 segundos antes de fijarse.

### ¿Cómo decide LightSpeed dónde añadir relays?

El proxy deriva el **país** de una dirección IP a partir de una base de datos DB-IP Lite almacenada localmente. Esto ocurre en memoria, de forma transitoria, al crear la sesión, tanto para la dirección de origen del cliente como para la de destino del servidor de juego. Luego cuenta sesiones por cada par `(source_country, destination_country)`, para que la red pueda ver qué pares de regiones están desatendidos. Una celda se suprime hasta que acumula al menos 3 sesiones antes de exportarse, y las estadísticas públicas solo llevan recuentos de pares de regiones generalizados (por ejemplo `mena-eu`). Los contadores no contienen ninguna IP en crudo, y el pipeline de colocación no exporta ninguna IP en crudo.

---

## FEC (reparación de paquetes)

### ¿Qué es la FEC?

Forward Error Correction. El relay envía una pequeña cantidad de datos redundantes
(hasta ~25% con el tamaño de bloque por defecto) junto a tus paquetes. Si se pierde
uno, se puede reconstruir a partir de la paridad sin retransmisión, así que la
recuperación no necesita un viaje de ida y vuelta al servidor de juego.

### ¿Necesito activarla?

No. La reparación de paquetes está activada por defecto y se adapta a la pérdida medida: una línea
limpia no soporta prácticamente ningún sobrecoste, y la paridad solo sube mientras los paquetes se
están perdiendo de verdad. (Las compilaciones más antiguas exponían un interruptor manual, el "Reliability
Shield"; desde la 1.7 la política adaptativa manda siempre.)

---

## Ejecutar un proxy

### ¿Cómo consigo un nodo proxy?

No tienes que hacer nada. LightSpeed incluye la red comunitaria de relays como opción por defecto: ocho relays financiados por patrocinadores (Los Angeles, New Jersey, Singapore, Frankfurt, Tokyo, Mumbai, Madrid, Sydney) que el cliente descubre automáticamente mediante un registro firmado. La URL del registro y la clave pública del operador van compiladas en el cliente, así que no hay nada que configurar ni archivo de configuración que crear.

Si quieres usar otro registro, cámbialo con `--registry <url>` o con un bloque `[registry]` en `lightspeed.toml`. Consulta la [guía de la red comunitaria de relays](community-network.md).

### ¿Necesito ejecutar mi propio proxy?

No. La red comunitaria es la opción por defecto y funciona recién instalada. Aun así, el autoalojamiento sigue teniendo soporte completo si quieres tu propio relay dedicado: despliega un proxy ligero (~500KB de RAM) en cualquier VPS con Linux. Consulta la [guía de despliegue](../infra/README.md).

### ¿Cuánto cuesta un proxy?

Nada si usas la red comunitaria. Si lo alojas tú, un VPS pequeño cuesta unos pocos dólares al mes, y el binario del proxy usa ~500KB de RAM, así que hasta la instancia más pequeña va sobrada. LightSpeed en sí no tiene ninguna tarifa.

### ¿Puedo compartir mi proxy con amigos?

Sí. El proxy admite varias sesiones concurrentes con límite de tasa por cliente y autenticación. Configura los tokens en `proxy.toml`.

---

## Resolución de problemas

### "No game traffic seen"

- Asegúrate de que tu juego esté realmente conectado a un servidor (no solo en el menú principal)
- Verifica que seleccionaste el juego correcto (flag `--game`)
- Prueba `--scan-processes` para listar los procesos de juego en ejecución
- Si tu servidor usa un puerto no estándar, usa el modo de servidor manual (`--game-server`)

### "Interceptor not available"

- Linux: asegúrate de ejecutar como root y de tener nftables/iptables instalado
- macOS: pfctl viene integrado pero requiere root
- Windows: comprueba que `WinDivert64.sys` y `WinDivert.dll` estén junto al `.exe`

### Paquetes enviados pero no entregados

Tus paquetes llegan al proxy pero las respuestas no llegan a tu juego. Normalmente es un problema de firewall. LightSpeed intenta añadir reglas de firewall automáticamente. Si falla, añade a mano una regla UDP de entrada para `lightspeed` o `lightspeed-gui.exe`.

---

## Privacidad

### ¿LightSpeed lee mi tráfico de juego?

LightSpeed ve las cabeceras de los paquetes UDP (IP de origen/destino, puerto, tamaño) para enrutarlos. El contenido del juego (posiciones de los jugadores, chat, etc.) va cifrado por el propio protocolo del juego y no se descifra ni se registra. Consulta la [Política de Privacidad](privacy.md) completa.

### ¿Hay telemetría?

La telemetría está **activada por defecto** desde la v1.6.5. Envía métricas agregadas anonimizadas (percentiles de RTT, jitter, estadísticas de FEC y los números de latencia directa/por relay/ahorrada) al endpoint `/telemetry` del relay al que estás conectado (un relay comunitario o de patrocinador, o el tuyo propio si lo alojas tú). No se recopilan direcciones IP, tokens, identificadores ni datos de cuentas de juego. Una celda se suprime hasta que acumula al menos 3 informes; ese mínimo cuenta informes, no personas distintas, así que no garantiza que hayan contribuido 3 personas diferentes. Los informes se envían por POST sobre HTTP en texto plano al puerto 8080, y el endpoint no está autenticado. Desactívala cuando quieras con `--no-telemetry`, con `telemetry = false` bajo `[general]` en `lightspeed.toml`, o con la casilla **"Share anonymous latency stats"** de la GUI. Consulta la [Política de Privacidad](privacy.md) y el [Diccionario de datos](data-dictionary.md).

### ¿El proxy guarda mi dirección IP?

No. El proxy procesa tu IP de origen y la IP de destino del servidor de juego de forma transitoria, en memoria, para enrutar los paquetes y para derivar un país con fines de análisis de colocación. La dirección en sí no se guarda. Lo que se conserva es un contador agregado de sesiones por cada par `(source_country, destination_country)`, con un mínimo de 3 sesiones por celda para que las celdas pequeñas se supriman, y las estadísticas públicas solo llevan recuentos de pares de regiones generalizados. Los contadores de colocación no contienen ninguna IP en crudo, y el pipeline de colocación no exporta ninguna IP en crudo; el contrato de telemetría del cliente no cambia. Ojo: los logs de acceso del relay pueden contener IPs de clientes; consulta [¿Qué registra el proxy?](#what-does-the-proxy-log).

### ¿Qué registra el proxy?

Los logs operativos escritos en stdout (configurables con `RUST_LOG`) pueden incluir IPs de clientes, horas de inicio/fin de sesión y bytes retransmitidos. Las IPs de clientes aparecen ahí solo para el límite de tasa y la detección de abuso. Esos logs viven en el relay que sirvió la sesión (un relay comunitario o de patrocinador, o el tuyo propio si lo alojas tú), no se exportan y no se cruzan con los contadores de colocación. La retención la controla la configuración de logging de ese relay. Consulta la [Política de Privacidad](privacy.md).

---

## Otros

### ¿Puedo usar LightSpeed con una VPN?

Por lo general no: ambos intentan interceptar el tráfico de red y entrarán en conflicto. Desactiva tu VPN antes de usar LightSpeed.

### ¿LightSpeed funciona con Cloudflare WARP?

Sí. Usa `--warp` para activar WARP en el tramo de proxy de la conexión. WARP puede recortar 5-10ms del enrutamiento local del ISP. Combínalo con un proxy para sacarle el máximo partido.

### ¿Dónde informo de bugs?

[Abre un issue en GitHub](https://github.com/ShibbityShwab/lightspeed/issues). Incluye tu sistema operativo, tu juego y la salida del log (ejecuta con `RUST_LOG=debug` para logs detallados).
