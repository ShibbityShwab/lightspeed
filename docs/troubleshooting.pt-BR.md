# Solução de problemas

---

## Diagnóstico rápido

Rode primeiro a verificação de ambiente embutida - ela pega a maioria dos problemas:

```bash
lightspeed --check
```

Isso verifica: disponibilidade do interceptor, ferramentas de filtragem de pacotes, resolução do perfil de jogo e conectividade com o proxy.

---

## Problemas comuns

### "Interceptor not available"

**CLI:** Seu sistema não tem as ferramentas de filtragem de pacotes necessárias ou você não tem privilégios.

| SO | Necessário | Como corrigir |
|----|----------|------------|
| Linux | nftables ou iptables + root | `sudo lightspeed ...` |
| macOS | pfctl (embutido) + root | `sudo lightspeed ...` |
| Windows | driver WinDivert + Administrador | Clique com o botão direito → Run as Administrator |

Verifique com:
```bash
lightspeed --check
```

### "No game traffic seen" / Packets Sent continua em 0

**CLI:** O interceptor não encontra pacotes do jogo na faixa de portas esperada.

1. Garanta que seu jogo esteja **conectado a um servidor** (não só no menu principal ou no lobby)
2. Verifique o jogo com `--scan-processes`:
   ```bash
   lightspeed --scan-processes
   ```
3. Tente um perfil de jogo diferente ou use o modo de servidor manual

**GUI (Windows):** Espere 15 segundos. Se a faixa âmbar "⚠ No game traffic seen" aparecer:
1. Abra o PowerShell elevado:
   ```powershell
   Get-NetUDPEndpoint -OwningProcess (Get-Process RustClient).Id |
     Where-Object LocalPort -gt 1024 |
     Select-Object LocalPort
   ```
2. Use a porta mostrada em **Advanced → set server manually**

### "🎯 Finding your game server…" nunca resolve

O detector não viu 3 pacotes para o mesmo destino dentro de 1.5 segundos.

1. Garanta que você está conectado a um servidor de jogo (mova seu personagem para gerar tráfego)
2. Se os pacotes ainda não forem detectados depois de 15 segundos, seu servidor está em uma porta não padrão - use o modo de servidor manual
3. Pare e reinicie o interceptor depois de se conectar ao servidor

### Packets Sent subindo, Packets Delivered = 0

Os pacotes chegam ao proxy, mas as respostas não chegam ao seu jogo. Normalmente é um problema de firewall.

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

### A verificação de saúde do proxy falha

```bash
# Test connectivity
curl http://YOUR_PROXY_IP:8080/health

# Expected response:
# {"status":"ok","node_id":"relay-1","uptime_secs":86400,...}
```

Se estiver inacessível:
- Confira se o proxy está rodando: `systemctl status lightspeed-proxy`
- Confira se o firewall permite UDP 4434 e TCP 8080
- Confira os logs do proxy: `journalctl -u lightspeed-proxy --tail 50`

### O jogo desconecta quando o interceptor inicia

O interceptor captura os pacotes antes que o jogo possa receber respostas, e o caminho de injeção falha.

**Windows:**
1. Verifique se `WinDivert64.sys` e `WinDivert.dll` estão ao lado do `.exe`
2. Desconecte adaptadores de rede secundários (adaptadores virtuais Docker, VMware, Hamachi)
3. Conecte-se ao servidor de jogo **antes** de iniciar o interceptor

**Linux:**
1. Confira as regras do nftables: `sudo nft list ruleset | grep lightspeed`
2. Se as regras estiverem obsoletas: `sudo lightspeed --check` para diagnosticar

### "WinDivert open failed" / `FWP_E_IN_USE` (0x8032000A) no Windows

O WinDivert registra um callout/filtro WFP para cada handle aberto. Se um handle nunca for fechado, esse estado de filtro permanece, e o próximo `WinDivertOpen` falha com `FWP_E_IN_USE` (0x8032000A) mesmo que nenhum processo esteja visivelmente usando o WinDivert.

Builds recentes do LightSpeed fecham tanto o handle de captura quanto o de injeção em todo caminho de encerramento ordeiro, incluindo Ctrl+C em `--watch` e `--start-interceptor` e **Quit** na GUI, e o loop de recebimento é desbloqueado com `WinDivertShutdown` antes do fechamento, para que o teardown seja determinístico. Um kill forçado (`taskkill /f`, um crash ou fechar a janela do console) ainda pode deixar o driver WinDivert 2.2.x com estado obsoleto; essa é uma limitação do driver a montante (basil00/WinDivert#294, #406) que o espaço de usuário não consegue limpar depois que o processo se foi.

Se você ainda esbarrar nisso:

1. **Saia de forma ordeira e espere um instante**: use Ctrl+C na CLI ou o **Quit** da GUI, depois dê um ou dois segundos para os handles fecharem antes de reabrir.
2. **Pare o serviço do WinDivert** (evita um reboot em alguns casos; note que o driver é compartilhado com qualquer outro app baseado em WinDivert na máquina):
   ```powershell
   sc stop windivert
   ```
3. **Desligamento completo, não reinicialização**: o "Restart" do Windows pode reutilizar a sessão de kernel que guarda o estado obsoleto; um **Desligar → ligar** completo limpa isso.

> **Dica:** Na v1.2.2 e anteriores, um bug separado (a autenticação do plano de dados rejeitando todos os pacotes, issue #59) congelava a conexão e obrigava os usuários a matar o cliente repetidamente, o que era o gatilho da maioria dos relatos de `FWP_E_IN_USE`. Esse bug de autenticação foi corrigido na v1.2.3.

---

## Problemas na GUI do Windows

Estes valem para o app `lightspeed-gui` no Windows.

### Quit não fez nada e deixou um processo zumbi

Em versões anteriores à v1.4.2, escolher **Quit** no menu da bandeja podia deixar o processo rodando em segundo plano (um zumbi), então a janela fechava, mas o motor continuava rodando e uma execução posterior se comportava de forma estranha. A v1.4.2 corrige isso: agora o Quit encerra o processo de forma limpa.

Se você está em um build mais antigo e o processo está travado, encerre-o manualmente:

```powershell
Get-Process lightspeed-gui -ErrorAction SilentlyContinue | Stop-Process
```

Depois atualize para a v1.4.2 ou posterior.

### Uma segunda instância abre em vez de focar a primeira

Em versões anteriores à v1.4.2, abrir a GUI duas vezes empilhava uma segunda janela e um segundo motor. Agora a GUI mantém uma trava de instância única: uma segunda execução mostra um aviso breve "LightSpeed is already running" e sai em vez de iniciar outro motor. Para forçar uma segunda instância de qualquer forma (para diagnóstico), passe `--force` ou defina `LIGHTSPEED_GUI_FORCE=1`.

### Nenhum relay descoberto

A GUI descobre os relays da comunidade pelo registro assinado. Se a lista de relays continuar vazia:

1. Confirme que você tem acesso à internet e que um firewall ou VPN não está bloqueando HTTPS de saída para o registro.
2. Confira o log da GUI (veja abaixo) para ver se há erro de busca do registro ou de verificação de assinatura.
3. No build da CLI, rode `lightspeed-client --probe-proxies` para ver o relatório de descoberta e sondagem diretamente. Se a CLI também não encontrar nada, o problema é do lado da rede, não específico da GUI.
4. Reinicie a GUI depois de corrigir a conectividade; a descoberta roda na inicialização.

### Como encontrar e abrir o log da GUI

A GUI grava o log de trace em:

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

Cole esse caminho na barra de endereços do Explorador de Arquivos para abrir a pasta e então abra `gui-trace.log` em qualquer editor de texto. Anexe-o a um relatório de bug.

### "Heartbeat 0 in" no log

Uma linha como `Heartbeat 0 in` significa que o motor enviou zero heartbeats de keepalive na janela atual. Na prática, ela aparece quando o cliente ainda não estabeleceu uma conexão funcional com o control plane, então nenhum heartbeat saiu. Causas comuns:

- O cliente ainda não se registrou com um relay (confira a linha de registro na tela de status).
- O relay selecionado está inacessível.
- O interceptor não iniciou, então nenhuma sessão está ativa.

Quando o registro dá certo e os heartbeats começam a fluir, o contador sobe. Se ele continuar em 0 enquanto um relay aparece saudável, rode `lightspeed-client --test-control` para isolar se o control plane está acessível.

---

## Logs para relatórios de bug

Rode com log de debug para capturar diagnósticos detalhados:

```bash
# CLI
RUST_LOG=debug lightspeed --start-interceptor --game rust --proxy YOUR_PROXY:4434 2>&1 | tee lightspeed.log

# Windows GUI
cd C:\path\to\lightspeed
lightspeed-gui.exe 2>&1 | tee lightspeed-log.txt
```

Anexe o arquivo de log à sua [issue no GitHub](https://github.com/ShibbityShwab/lightspeed/issues).

---

## Ainda travado?

- [Perguntas frequentes](faq.md) - dúvidas comuns
- [Issues no GitHub](https://github.com/ShibbityShwab/lightspeed/issues) - pesquise relatos existentes
- Abra uma nova issue com seu sistema operacional, o jogo e a saída do log
