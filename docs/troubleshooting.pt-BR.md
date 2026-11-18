# Solução de problemas

> [!WARNING]
> Tradução assistida por máquina, não revisada por um falante nativo. A [versão em inglês](troubleshooting.md) é a autoritativa.

---

## Diagnóstico Rápido

Execute primeiro a verificação do ambiente integrado - ela detecta a maioria dos problemas:

```bash
lightspeed --check
```

Isso verifica: a disponibilidade do interceptador, as ferramentas de filtragem de pacotes, a resolução do perfil do jogo e a conectividade do proxy.

---

## Problemas comuns

### "Interceptor não disponível"

**CLI:** seu sistema operacional não possui as ferramentas de filtragem de pacotes necessárias ou você não tem privilégios.

| SO | Obrigatório | Como consertar |
|----|----------|-----------|
| Linux | nftables ou iptables + root | `sudo lightspeed ...` |
| macOS | pfctl (integrado) + raiz | `sudo lightspeed ...` |
| Janelas | Driver WinDivert + Administrador | Clique com o botão direito → Executar como administrador |

Verifique com:
```bash
lightspeed --check
```

### "Nenhum tráfego de jogo visto" / Pacotes enviados permanecem em 0

**CLI:** O interceptor não consegue encontrar pacotes de jogos no intervalo de portas esperado.

1. Certifique-se de que seu jogo esteja **conectado a um servidor** (não apenas ao menu principal ou lobby)
2. Verifique o jogo com `--scan-processes`:
   ```bash
   lightspeed --scan-processes
   ```
3. Experimente um perfil de jogo diferente ou use o modo de servidor manual

**GUI (Windows):** Aguarde 15 segundos. Se o banner âmbar "⚠ Nenhum tráfego de jogo visto" aparecer:
1. Abra o PowerShell elevado:
   ```powershell
   Get-NetUDPEndpoint -OwningProcess (Get-Process RustClient).Id |
     Where-Object LocalPort -gt 1024 |
     Select-Object LocalPort
   ```
2. Use a porta mostrada em **Avançado → configurar servidor manualmente**

### "🎯 Encontrando seu servidor de jogo…" nunca resolve

O detector não viu 3 pacotes para o mesmo destino em 1,5 segundos.

1. Certifique-se de estar conectado a um servidor de jogo (mova seu personagem para gerar tráfego)
2. Se os pacotes ainda não forem detectados após 15 segundos, seu servidor está em uma porta não padrão - use o modo de servidor manual
3. Pare e reinicie o interceptor após conectar-se ao servidor

### Pacotes enviados subindo, Pacotes entregues = 0

Os pacotes chegam ao proxy, mas as respostas não chegam ao seu jogo. Geralmente um problema de firewall.

**Linux:**
```bash
sudo iptables -I INPUT -p udp --sport 4434 -j ACCEPT
```

**macOS:**
```bash
sudo pfctl -d  # Temporarily disable pf to test
```

**Janelas:**
```powershell
# Check if the stable rule exists (one rule, not one per boost)
netsh advfirewall firewall show rule name="LightSpeed WinDivert Tunnel"

# Add it manually if missing
netsh advfirewall firewall add rule name="LightSpeed WinDivert Tunnel" protocol=UDP dir=in action=allow program="C:\path\to\lightspeed-gui.exe"
```

As regras de compilações mais antigas foram denominadas `LightSpeed WinDivert Tunnel <port>`; atual
builds removem aquele da porta que usam e mantêm uma única regra estável. Qualquer
as regras restantes por porta podem ser excluídas com segurança:
`netsh advfirewall firewall delete rule name="LightSpeed WinDivert Tunnel 4434"`
(corresponde à porta mostrada por `netsh advfirewall firewall show rule name=all`).

### `sendmsg error: ... 10040` / `WSAEMSGSIZE` (Windows) ou `EMSGSIZE` (Linux/macOS)

O datagrama que o cliente tentou enviar é maior que o MTU do caminho **real**. O
o limite de túnel padrão é 1492 bytes (seguro para PPPoE), mas uma VPN, um calço de DPI como
Zapret, ou um adaptador de MTU reduzido, pode ser menor, portanto, um pacote que caiba no grampo
ainda é rejeitado pelo sistema operacional (`Windows error 10040`).

Correção – reduza o limite MTU do caminho do túnel, em `config.toml`:

```toml
[tunnel]
# 1280-1500. Start at 1400, then 1350 if the error persists.
mtu = 1400
```

ou passe `--mtu 1400` na CLI. As construções atuais também reduzem o orçamento
automaticamente quando um envio for rejeitado, tente novamente esse datagrama uma vez com
fragmentação permitida e registre o erro do sistema operacional no status em vez de
relatando o relé como silencioso.

Solução alternativa imediata enquanto o cliente permanece inalterado: diminua o adaptador do Windows
MTU para que o jogo emita pacotes menores e reinicie o jogo:

```powershell
netsh interface ipv4 show subinterfaces
netsh interface ipv4 set subinterface "Ethernet" mtu=1400 store=persistent
```

### Falha na verificação de integridade do proxy

```bash
# Test connectivity
curl http://YOUR_PROXY_IP:8080/health

# Expected response:
# {"status":"ok","node_id":"relay-1","uptime_secs":86400,...}
```

Se inacessível:
- Verifique se o proxy está em execução: `systemctl status lightspeed-proxy`
- Verifique se o firewall permite UDP 4434 e TCP 8080
- Verifique os logs do proxy: `journalctl -u lightspeed-proxy --tail 50`

### O plano de controle QUIC falha com `error 120` / `no_application_protocol`

O cliente não pôde concluir o handshake do plano de controle QUIC com um relé. Quando o
A nova tentativa única de oferta vazia também falhou, o log termina com este veredicto (citado
literalmente do cliente; `{err}` é o erro QUIC subjacente e `{addr}` o
endereço do respondente):

```text
{err}: the responder at {addr} closed the QUIC handshake with TLS alert 120 (no_application_protocol), and the empty-offer retry also failed. The client cannot tell these two causes apart from the handshake alone: (1) a QUIC-intercepting middlebox on this path (VPN/AV/ISP) that does not carry the ALPN extension (see #140); or (2) a LightSpeed relay on a different ALPN revision -- version skew, where the relay requires an ALPN name this client does not offer.
```

**O que o cliente já fez.** Os clientes atuais sempre oferecem o nome ALPN versionado
`lightspeed-control/1`. Se o relé responder com o alerta TLS 120, o cliente tentará novamente
**uma vez** com uma oferta ALPN vazia e depois lembra a resposta daquele relé para o resto
do processo, portanto, na operação normal do cliente, o aperto de mão extra condenado acontece em
mais uma vez por revezamento por lançamento. Um fallback bem-sucedido é normal contra um relé que
não foi atualizado (o padrão do relé é `[network] control_alpn = "off"`): o
A linha `QUIC connection established` mostra `alpn=none` e o registro prossegue.

**O que significa a falha final.** A nova tentativa de oferta vazia também falhou, então há
exatamente duas possibilidades e o aperto de mão por si só não pode diferenciá-las:

1. **Um middlebox de interceptação QUIC em seu caminho** - uma VPN, antivírus/firewall com
   Inspeção HTTPS/QUIC ou equipamento ISP que não carrega a extensão ALPN
   ([#140](https://github.com/ShibbityShwab/lightspeed/issues/140)). Se cada relé
   falha em uma rede, mas os mesmos relés funcionam em outra, o caminho é o comum
   fator.
2. **Distorção de versão** - o relé requer uma revisão ALPN que este cliente não oferece
   (por exemplo, um relé definido como `control_alpn = "required"` e executando um protocolo
   revisão mais recente que este cliente). Isso falha ruidosamente por design, nunca como um silêncio
   downgrade: um cliente atual contra um relé em `lightspeed-control/1` negocia,
   e um relé que exige algo que este cliente não oferece rejeita com 120.

**Como restringir:**

- Execute `lightspeed-client --test-control --proxy <relay-ip>` para ver em qual estágio esse
  o relé está em. O valor `alpn=` na linha conectada e no resumo final é o
  protocolo de aplicação negociado: `lightspeed-control/1` significa que o relé aceitou o
  oferta versionada, `none` significa que aceitou uma oferta vazia após o substituto (uma oferta mais antiga
  retransmissão), e uma execução que falha com o veredicto desta seção é o caso acima.
- Se todos os relés falharem nesta rede, mas os mesmos relés funcionarem em outro lugar, suspeite do
  middlebox: desative a VPN ou a inspeção HTTPS/QUIC do produto de segurança e tente novamente,
  em seguida, anexe uma captura UDP 4433 ao #140.
- Se apenas um relé falhar, compare o modo ALPN desse relé com a construção do seu cliente: o
  linha de inicialização do relé e o campo `control_alpn` em seu `GET /health` relatam o
  modo configurado (consulte [Referência CLI](CLI-REFERENCE.md#proxy-configuration)). Se você
  operar esse relé, `control_alpn = "off"` restaura o comportamento anterior para dispositivos mais antigos
  clientes; o procedimento do operador e a regra de ordenação do cliente primeiro estão em
  [Deploy Proxy](deploy-proxy.md) e o [plano de implementação do ALPN](quic-alpn-rollout.md).

### O jogo é desconectado quando o interceptor é iniciado

O interceptador captura pacotes antes que o jogo possa receber respostas e o caminho de injeção falha.

**Janelas:**
1. Verifique se `WinDivert64.sys` e `WinDivert.dll` estão próximos ao `.exe`
2. Desconecte os adaptadores de rede secundários (adaptadores virtuais Docker, VMware, Hamachi)
3. Conecte-se ao servidor do jogo **antes** de iniciar o interceptador

**Linux:**
1. Verifique as regras do nftables: `sudo nft list ruleset | grep lightspeed`
2. Se as regras estiverem obsoletas: `sudo lightspeed --check` para diagnosticar

### "Falha na abertura do WinDivert" / `FWP_E_IN_USE` (0x8032000A) no Windows

WinDivert registra uma chamada/filtro WFP para cada identificador aberto. Se um identificador nunca for fechado, esse estado de filtro permanecerá e o próximo `WinDivertOpen` falhará com `FWP_E_IN_USE` (0x8032000A), mesmo que nenhum processo esteja visivelmente usando o WinDivert.

Compilações recentes do LightSpeed ​​fecham as alças de captura e injeção em cada caminho de desligamento ordenado, incluindo Ctrl+C em `--watch` e `--start-interceptor` e **Quit** na GUI, e o loop de recebimento é desbloqueado com `WinDivertShutdown` antes do fechamento, portanto a desmontagem é determinística. Um hard kill (`taskkill /f`, uma falha ou fechamento da janela do console) ainda pode deixar o driver WinDivert 2.2.x com estado obsoleto; essa é uma limitação do driver upstream (basil00/WinDivert#294, #406) que o espaço do usuário não pode limpar depois que o processo termina.

Se você ainda acertar:

1. **Encerre normalmente e espere um momento**: use a CLI Ctrl+C ou **Quit** da GUI e aguarde um ou dois segundos para que as alças fechem antes de reiniciar.
2. **Pare o serviço WinDivert** (evita uma reinicialização em alguns casos; observe que o driver é compartilhado com qualquer outro aplicativo baseado em WinDivert na máquina):
   ```powershell
   sc stop windivert
   ```
3. **Desligamento total, não reinicialização**: "Reiniciar" do Windows pode reutilizar a sessão do kernel que mantém o estado obsoleto; um **Desligamento → ligar** completo o limpa.

> **Dica:** Na versão 1.2.2 e anteriores, um bug separado (autenticação do plano de dados rejeitando todos os pacotes, problema nº 59) congelou a conexão e forçou os usuários a matar o cliente repetidamente, que foi o que acionou a maioria dos relatórios `FWP_E_IN_USE`. Esse bug de autenticação foi corrigido na v1.2.3.

---

## Problemas de GUI do Windows

Isso se aplica ao aplicativo `lightspeed-gui` no Windows.

### Quit não fez nada e deixou um processo zumbi

Nas versões anteriores à v1.4.2, escolher **Sair** no menu da bandeja poderia deixar o processo rodando em segundo plano (um zumbi), então a janela fechava, mas o mecanismo continuava funcionando e uma inicialização posterior se comportava de maneira estranha. v1.4.2 corrige isso: Sair agora encerra o processo de forma limpa.

Se você estiver em uma versão mais antiga e o processo travar, finalize-o manualmente:

```powershell
Get-Process lightspeed-gui -ErrorAction SilentlyContinue | Stop-Process
```

Em seguida, atualize para a versão 1.4.2 ou posterior.

### Uma segunda instância é aberta em vez de focar a primeira

Nas versões anteriores à v1.4.2, iniciar a GUI empilhava duas vezes uma segunda janela e um segundo mecanismo. A GUI agora mantém uma proteção de instância única: uma segunda inicialização mostra um breve aviso “LightSpeed ​​​​já está em execução” e sai em vez de iniciar outro mecanismo. Para forçar uma segunda instância de qualquer maneira (para diagnóstico), passe `--force` ou defina `LIGHTSPEED_GUI_FORCE=1`.

### Nenhum relé descoberto

A GUI descobre as retransmissões da comunidade por meio do registro assinado. Se a lista de retransmissão permanecer vazia:

1. Confirme se você tem acesso à Internet e se um firewall ou VPN não está bloqueando o HTTPS de saída para o registro.
2. Verifique o log da GUI (veja abaixo) para obter um erro de busca de registro ou de verificação de assinatura.
3. Na construção da CLI, execute `lightspeed-client --probe-proxies` para ver diretamente o relatório de descoberta e investigação. Se a CLI também não encontrar nada, o problema será do lado da rede e não específico da GUI.
4. Reinicie a GUI após corrigir a conectividade; a descoberta é executada na inicialização.

### Como encontrar e abrir o log da GUI

A GUI grava seu log de rastreamento em:

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

Cole esse caminho na barra de endereço do File Explorer para abrir a pasta e abra `gui-trace.log` em qualquer editor de texto. Anexe-o a um relatório de bug.

### "Heartbeat 0 in" no log

Uma linha como `Heartbeat 0 in` significa que o mecanismo não enviou nenhuma pulsação de manutenção de atividade na janela atual. Na prática, isso aparece quando o cliente ainda não estabeleceu uma conexão de plano de controle funcional, portanto, nenhuma pulsação ocorreu. Causas comuns:

- O cliente ainda não se registrou em um relé (verifique a linha de registro na visualização de status).
- O relé selecionado está inacessível.
- O interceptor não foi iniciado, portanto nenhuma sessão está ativa.

Assim que o registro for bem-sucedido e os batimentos cardíacos começarem a fluir, o contador sobe. Se permanecer em 0 enquanto um relé estiver íntegro, execute `lightspeed-client --test-control` para isolar se o plano de controle está acessível.

---

## Logs para relatórios de bugs

Execute com registro de depuração para capturar diagnósticos detalhados:

```bash
# CLI
RUST_LOG=debug lightspeed --start-interceptor --game rust --proxy YOUR_PROXY:4434 2>&1 | tee lightspeed.log

# Windows GUI
cd C:\path\to\lightspeed
lightspeed-gui.exe 2>&1 | tee lightspeed-log.txt
```

Anexe o arquivo de log ao seu [problema do GitHub](https://github.com/ShibbityShwab/lightspeed/issues).

---

## Ainda preso?

- [FAQ](faq.md) - perguntas comuns
- [Problemas do GitHub](https://github.com/ShibbityShwab/lightspeed/issues) - pesquise relatórios existentes
- Abra um novo problema com seu sistema operacional, jogo e saída de log