# Perguntas frequentes

---

## Noções básicas

### O LightSpeed é realmente gratuito?

Sim. O LightSpeed é gratuito para uso pessoal e não comercial sob a Licença de Software LightSpeed. O uso comercial exige uma licença paga - veja [LICENSE](../LICENSE). Você roda seu próprio proxy em uma VPS pequena (veja o [guia de implantação](../infra/README.md)). Não há assinaturas, nem taxas de uso, nem planos pagos.

### O LightSpeed vai me banir?

Não. O LightSpeed usa a mesma classe de driver de rede em nível de sistema (WinDivert/nftables/pfctl) que outras ferramentas de captura de pacotes usam. Ele não modifica arquivos, memória ou processos do jogo. Todos os principais sistemas anti-cheat (EAC, VAC, BattlEye, Riot Vanguard) permitem isso. Os servidores de jogo veem seu endereço IP real - isso é um túnel transparente, não uma VPN ou anonimizador.

### Por que o interceptor precisa de root/Administrador?

A interceptação de pacotes em nível de kernel exige privilégios elevados - o mesmo motivo pelo qual VPNs e firewalls precisam deles. No Linux usa nftables/iptables. No macOS usa pfctl. No Windows usa WinDivert (um driver de kernel assinado). Sem root, você ainda pode usar o modo redirect (`--game-server`).

### Quais plataformas são suportadas?

| Plataforma | Interceptor | Modo redirect | GUI |
|----------|-------------|---------------|-----|
| Windows 10/11 | ✅ WinDivert | ✅ | ✅ egui |
| Linux | ✅ nftables/iptables | ✅ | ❌ CLI only |
| macOS | ✅ pfctl | ✅ | ❌ CLI only |
| Linux ARM64 | ✅ | ✅ | ❌ |

---

## Como funciona

### Como o LightSpeed realmente reduz o ping?

O LightSpeed **não** faz seu tráfego ir mais rápido - pacotes não podem ultrapassar a velocidade da luz. O que ele faz é *rotear de forma proativa* seu tráfego pelo caminho mais rápido disponível, evitando congestionamento e desvios desnecessariamente longos.

Seu provedor envia pacotes pelo caminho que for mais barato para *ele* - muitas vezes congestionado ou tortuoso. O LightSpeed envia seus pacotes por um proxy em um grande data center com conexões diretas de backbone para as regiões dos servidores de jogo. Se esse caminho for mais curto ou menos congestionado que a rota padrão do seu provedor, seu ping cai e se estabiliza. Melhoria típica: 10-40ms.

### Meu ping AUMENTOU. Por quê?

Dois motivos comuns:

1. **Local do proxy errado** - se o proxy estiver mais longe do servidor de jogo do que o seu caminho direto, o salto extra adiciona latência. Essa é a causa mais comum. Regra geral: escolha o proxy mais perto do **servidor de jogo**, não o mais perto de você.
2. **Proxy mal conectado** - nem todos os data centers são iguais. Um proxy só ajuda se aquele data center estiver perto de um backbone de internet importante ou de um ponto de peering. Uma VPS barata na cidade "certa", mas em um upstream congestionado ou residencial, pode ser mais lenta que a sua rota direta.

O LightSpeed só consegue otimizar a rota que recebe. Se você apontar para um proxy mal posicionado, seu ping vai subir - isso é comportamento esperado, não um bug.

### Qual proxy devo escolher?

O proxy mais perto da **região do servidor de jogo**. Exemplos:
- Jogando em servidores US West → escolha um proxy US West
- Jogando em servidores de Singapura a partir da Austrália → escolha um proxy em Singapura
- Jogando em servidores da UE a partir da América do Norte → escolha um proxy em Frankfurt/London

### Uma nota sobre a realidade do roteamento (BGP)

O roteamento real da internet é governado pelo **BGP** (Border Gateway Protocol) - os contratos e políticas que provedores e operadoras de trânsito usam para repassar tráfego. Seus pacotes não viajam em linha reta; eles seguem o caminho que as tabelas BGP e os acordos de peering decidirem, e os provedores rotineiramente priorizam ou despriorizam certas rotas por motivos de custo ou política.

O que isso significa para você:

- Um proxy só ajuda se estiver em um caminho BGP *melhor* que o padrão da sua conexão de casa - normalmente um data center perto de um backbone importante ou de um ponto de peering.
- "Mais perto no mapa" nem sempre significa "mais rápido no fio."
- Ferramentas de otimização de rota (incluindo o LightSpeed) estimam e reroteiam, mas o caminho físico é, no fim das contas, ditado pelas redes intermediárias - que nem você nem o LightSpeed controlam.

### Quão rápida é a detecção automática?

Normalmente de 1 a 3 segundos depois que você se conecta a um servidor de jogo. O interceptor espera 3 pacotes para o mesmo destino dentro de 1.5 segundos antes de travar.

### Como o LightSpeed decide onde adicionar relays?

O proxy deriva o **país** de um endereço IP a partir de um banco de dados DB-IP Lite armazenado localmente. Isso acontece na memória, de forma transitória, na criação da sessão, tanto para o endereço de origem do cliente quanto para o endereço de destino do servidor de jogo. Depois ele conta sessões por par `(source_country, destination_country)`, para que a rede veja quais pares de regiões estão mal atendidos. Uma célula é suprimida até ter pelo menos 3 sessões antes de ser exportada, e as estatísticas públicas carregam apenas contagens de pares de regiões generalizadas (por exemplo `mena-eu`). Os contadores não contêm IP bruto, e nenhum IP bruto é exportado pelo pipeline de posicionamento.

---

## FEC (reparo de pacotes)

### O que é FEC?

Forward Error Correction. O relay envia uma pequena quantidade de dados
redundantes (até ~25% no tamanho de bloco padrão) junto com seus pacotes. Se um
for perdido, ele pode ser reconstruído a partir da paridade sem retransmissão, então
a recuperação não precisa de uma ida e volta até o servidor de jogo.

### Preciso ativar?

Não. O reparo de pacotes vem ligado por padrão e se adapta à perda medida: uma linha
limpa carrega efetivamente nenhum overhead, e a paridade sobe apenas enquanto os pacotes
estão realmente sendo perdidos. (Builds anteriores expunham um botão manual, o "Reliability
Shield"; desde a 1.7 a política adaptativa é quem manda.)

---

## Rodando um proxy

### Como consigo um nó de proxy?

Você não precisa fazer nada. O LightSpeed vem com a rede de relays da comunidade como padrão: oito relays mantidos por patrocinadores (Los Angeles, New Jersey, Singapore, Frankfurt, Tokyo, Mumbai, Madrid, Sydney) que o cliente descobre automaticamente por meio de um registro assinado. A URL do registro e a chave pública do operador são compiladas no cliente, então não há configuração nem arquivo de config necessário.

Se você quiser usar um registro diferente, sobrescreva com `--registry <url>` ou com um bloco `[registry]` em `lightspeed.toml`. Veja o [guia da Rede de Relays da Comunidade](community-network.md).

### Preciso rodar meu próprio proxy?

Não. A rede da comunidade é o padrão e funciona de imediato. Hospedar por conta própria continua totalmente suportado se você quiser seu próprio relay dedicado: implante um proxy leve (~500KB de RAM) em qualquer VPS Linux. Veja o [guia de implantação](../infra/README.md).

### Quanto custa um proxy?

Nada, se você usar a rede da comunidade. Se hospedar por conta própria, uma VPS pequena custa alguns dólares por mês, e o binário do proxy usa ~500KB de RAM, então até a menor instância dá conta. O LightSpeed em si não tem taxas.

### Posso compartilhar meu proxy com amigos?

Sim. O proxy suporta várias sessões simultâneas com limite de taxa e autenticação por cliente. Configure tokens em `proxy.toml`.

---

## Solução de problemas

### "No game traffic seen"

- Garanta que seu jogo esteja realmente conectado a um servidor (não só no menu principal)
- Verifique se você selecionou o jogo correto (flag `--game`)
- Tente `--scan-processes` para listar processos de jogos em execução
- Se seu servidor usa uma porta não padrão, use o modo de servidor manual (`--game-server`)

### "Interceptor not available"

- Linux: garanta que você está rodando como root e que nftables/iptables está instalado
- macOS: pfctl é embutido, mas exige root
- Windows: garanta que `WinDivert64.sys` e `WinDivert.dll` estejam ao lado do `.exe`

### Pacotes enviados, mas não entregues

Seus pacotes chegam ao proxy, mas as respostas não chegam ao seu jogo. Normalmente é um problema de firewall. O LightSpeed tenta adicionar regras de firewall automaticamente. Se isso falhar, adicione manualmente uma regra de entrada UDP para `lightspeed` ou `lightspeed-gui.exe`.

---

## Privacidade

### O LightSpeed lê meu tráfego de jogo?

O LightSpeed vê cabeçalhos de pacotes UDP (IP de origem/destino, porta, tamanho) para roteá-los. O conteúdo do jogo (posições dos jogadores, chat, etc.) é criptografado pelo próprio protocolo do jogo e não é descriptografado nem registrado em log. Veja a [Política de Privacidade](privacy.md) completa.

### Há telemetria?

A telemetria fica **ligada por padrão** desde a v1.6.5. Ela envia métricas agregadas anonimizadas (percentis de RTT, jitter, estatísticas de FEC e os números de latência direta/relayed/saved) para o endpoint `/telemetry` do relay ao qual você está conectado (um relay da comunidade ou de patrocinador, ou o seu próprio se você hospeda por conta própria). Nenhum endereço IP, token, identificador ou dado de conta de jogo é coletado. Uma célula é suprimida até ter pelo menos 3 relatórios; esse piso conta relatórios, não pessoas distintas, então não é garantia de que 3 pessoas diferentes contribuíram. Os relatórios são enviados por POST em HTTP sem criptografia para a porta 8080, e o endpoint não tem autenticação. Desligue quando quiser com `--no-telemetry`, com `telemetry = false` sob `[general]` em `lightspeed.toml`, ou com a caixa **"Share anonymous latency stats"** da GUI. Veja a [Política de Privacidade](privacy.md) e o [Dicionário de Dados](data-dictionary.md).

### O proxy armazena meu endereço IP?

Não. O proxy processa seu IP de origem e o IP de destino do servidor de jogo de forma transitória, na memória, para rotear pacotes e derivar um país para a análise de posicionamento. O endereço em si não é armazenado. O que fica guardado é um contador agregado de sessões por par `(source_country, destination_country)`, com um piso de 3 sessões por célula para que células pequenas sejam suprimidas, e as estatísticas públicas carregam apenas contagens de pares de regiões generalizadas. Os contadores de posicionamento não contêm IP bruto, e nenhum IP bruto é exportado pelo pipeline de posicionamento; o contrato de telemetria do cliente não muda. Note que os logs de acesso do relay podem conter IPs de clientes; veja [O que o proxy registra em log?](#what-does-the-proxy-log).

### O que o proxy registra em log?

Logs operacionais gravados em stdout (configuráveis via `RUST_LOG`) podem incluir IPs de clientes, horários de início/fim de sessão e bytes repassados. Os IPs de clientes aparecem ali apenas para limite de taxa e detecção de abuso. Esses logs ficam no relay que atendeu a sessão (um relay da comunidade ou de patrocinador, ou o seu próprio se você hospeda por conta própria), não são exportados e não são cruzados com os contadores de posicionamento. A retenção é controlada pela configuração de log daquele relay. Veja a [Política de Privacidade](privacy.md).

---

## Outros

### Posso usar o LightSpeed com uma VPN?

Em geral não - os dois tentam interceptar o tráfego de rede e vão entrar em conflito. Desative sua VPN antes de usar o LightSpeed.

### O LightSpeed funciona com o Cloudflare WARP?

Sim. Use `--warp` para ativar o WARP no trecho de proxy da conexão. O WARP pode economizar de 5 a 10ms no roteamento do provedor local. Combine com um proxy para o máximo de benefício.

### Onde reporto bugs?

[Abra uma issue no GitHub](https://github.com/ShibbityShwab/lightspeed/issues). Inclua seu sistema operacional, o jogo e a saída do log (rode com `RUST_LOG=debug` para logs detalhados).
