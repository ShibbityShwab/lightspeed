# Guia do usuário do LightSpeed

> [!WARNING]
> Tradução assistida por máquina, não revisada por um falante nativo. A [versão em inglês](user-guide.md) é a autoritativa.

> Instruções passo a passo para reduzir seu ping com o LightSpeed.

---

## Como o LightSpeed funciona

Seu provedor roteia o tráfego do jogo por caminhos otimizados para custo, não para velocidade. O LightSpeed intercepta os pacotes UDP do seu jogo e os tunelа por um **relay** - um servidor leve em um data center com conexões de backbone de alta velocidade para as regiões dos servidores de jogo. Por padrão você usa a **rede de relays da comunidade** (oito relays mantidos por patrocinadores, descobertos automaticamente via um registro assinado, sem nenhuma configuração). Você também pode hospedar seu próprio proxy. Se esse caminho for mais rápido que a rota padrão do seu provedor, seu ping cai.

```
Seu PC ──→ ISP (slow path) ──→ Game Server        ❌ High ping
Seu PC ──→ LightSpeed Proxy (fast backbone) ──→ Game Server   ✅ Low ping
```

---

## Pré-requisitos

- A ferramenta de linha de comando `lightspeed` ou o `lightspeed-gui` (Windows). Nenhuma configuração de proxy é necessária: o cliente descobre os relays da comunidade automaticamente.
- Para o modo interceptor: privilégios de root/Administrador
- Opcional: seu próprio nó de proxy se você preferir hospedar por conta própria (veja [Implantar Proxy](deploy-proxy.md))

---

## Qual app eu preciso?

| Você está em | Download | Por quê |
|-----------|----------|-----|
| **Windows** | `lightspeed-gui` (MSI ou ZIP) | A GUI é um app independente - ela já inclui o motor do cliente + o driver WinDivert. Você **não** precisa da CLI. |
| **Linux** | `lightspeed-gui` (ou `lightspeed-client`) | A GUI funciona no Linux (a bandeja do sistema é um stub); a CLI é para usuários avançados. |
| **macOS** | `lightspeed-client` | Ainda não há GUI testada. A GUI compila para macOS, mas não foi **testada** em hardware real. |
| **Hospedando um proxy** | `lightspeed-proxy` | Só se você estiver rodando um nó de relay em uma VPS. |

> **Você só precisa de um pacote.** Se você é jogador no Windows, pegue o `lightspeed-gui` e ignore o resto. O `lightspeed-client` é para usuários avançados de Linux e jogadores de macOS; o `lightspeed-proxy` é para quem hospeda por conta própria.

---

## Início rápido (CLI - Todas as plataformas)

### 1. Verifique seu ambiente

```bash
lightspeed --check
```

Isso verifica se o seu sistema tem as ferramentas de filtragem de pacotes necessárias (nftables/iptables no Linux, pfctl no macOS, WinDivert no Windows).

### 2. Sondar seus relays

```bash
lightspeed --probe-proxies
```

Mostra a latência de cada relay descoberto. O cliente seleciona automaticamente o mais rápido na primeira execução; você pode sobrescrever escolhendo o que estiver mais perto do seu **servidor de jogo**, não da sua localização.

### 3. Iniciar o interceptor

```bash
# Linux/macOS (requires root)
sudo lightspeed --start-interceptor --game rust --proxy YOUR_PROXY_IP:4434

# Windows (requires Administrator)
lightspeed --start-interceptor --game rust --proxy YOUR_PROXY_IP:4434
```

### 4. Abra seu jogo

Conecte-se a qualquer servidor normalmente. O LightSpeed detecta o servidor de jogo automaticamente a partir dos pacotes de saída e começa a tunelar em segundos.

### 5. Monitore

A CLI mostra estatísticas ao vivo:
```
⚡ OPTIMIZING - 123.45.67.89:28015
Packets Sent: 142 | Packets Returned: 139 | Packets Delivered: 139
```

---

## Início rápido (GUI - Windows)

### 1. Baixe

Pegue a versão mais recente em [Releases](https://github.com/ShibbityShwab/lightspeed/releases). Extraia todos os arquivos - mantenha `WinDivert64.sys` e `WinDivert.dll` ao lado do `lightspeed-gui.exe`.

### 2. Rode como Administrador

Clique com o botão direito em `lightspeed-gui.exe` → **Run as administrator**. O interceptor precisa de acesso em nível de kernel (o mesmo que softwares de VPN).

### 3. Escolha um relay e um jogo

A GUI descobre os relays da comunidade e seleciona o mais rápido automaticamente na primeira execução. Você pode sobrescrever o relay no menu suspenso, depois escolher seu jogo.

### 4. Clique em **⚡ OPTIMIZE MY ROUTE**

O status muda para "🎯 Finding your game server…"

### 5. Abra seu jogo

Conecte-se a qualquer servidor. O LightSpeed o detecta em segundos.

---

### GUI do macOS (não testada)

A GUI compila para macOS, mas não foi **testada** em hardware real. A release
traz apenas um `tar.xz` cru (o cargo-dist 0.32 não tem suporte a `.app`/`.dmg`), então para
produzir um bundle adequado, rode em um Mac:

```bash
cargo build --release -p lightspeed-gui
./tools/package-macos.sh 1.6.5
```

Isso cria `LightSpeed.app` e `LightSpeed-1.6.5.dmg`. O app tem assinatura
ad-hoc, então a primeira execução precisa de clique com o botão direito → Open (ou
`xattr -dr com.apple.quarantine LightSpeed.app`).

---

## Escolhendo o relay certo

| Você está em | Servidor de jogo em | Melhor região de relay |
|-----------|---------------|-------------------|
| Austrália | US West | US West (Los Angeles) |
| Europa | US East | US East (New Jersey) |
| Sudeste Asiático | Singapura | Singapura |
| Sul da Ásia | Índia | Mumbai |
| Leste Asiático | Japão | Tóquio |
| América do Sul | US East | US East (New Jersey) |
| Qualquer lugar | Mesma região | Mais perto do servidor de jogo |

> **Regra geral:** Escolha o relay mais perto do **servidor de jogo**, não o mais perto de você. Seu tráfego vai PC → relay → servidor de jogo, então o trecho relay-servidor de jogo é o que mais importa.

---

## Correção de erros de encaminhamento (FEC)

O FEC adiciona ~25% de overhead de banda para recuperar pacotes perdidos sem retransmissão.

**Ative quando:**
- Você tem perda de pacotes (micro-engasgos, rubber-banding)
- Você está no Wi-Fi com interferência intermitente

**Desative quando:**
- Sua conexão já está saturada
- Você está em uma conexão limitada ou com franquia
- Você tem < 0.1% de perda de pacotes (nenhum benefício)

```bash
# CLI: enable FEC with default block size (K=4)
lightspeed --start-interceptor --game cs2 --proxy YOUR_PROXY:4434 --fec

# Custom block size (K=8 → 12.5% overhead)
lightspeed --start-interceptor --game cs2 --proxy YOUR_PROXY:4434 --fec --fec-k 8
```

---

## Avançado: modo de servidor manual

Se a detecção automática não funcionar (portas personalizadas, jogo incomum):

```bash
# Redirect mode: game connects to localhost, LightSpeed forwards to real server
lightspeed --game rust --game-server 123.45.67.89:28015 --proxy YOUR_PROXY:4434
```

Depois configure seu jogo para se conectar a `127.0.0.1:<port>` (a porta local que o LightSpeed imprime).

---

## Trocando de servidor no meio da sessão

O LightSpeed detecta automaticamente quando você se desconecta de um servidor e se conecta a outro. O status mostra brevemente "🎯 Finding your game server…" e trava no novo destino. Nenhuma ação manual é necessária.

---

## Bandeja do sistema (GUI do Windows)

- Clique em **×** para minimizar para a bandeja (não sai do app)
- Dê um duplo clique no ícone do raio para restaurar
- Clique com o botão direito para Connect / Disconnect / Quit rápidos

---

## Veja também

- [Referência da CLI](CLI-REFERENCE.md) - cada flag explicada
- [Perguntas frequentes](faq.md) - dúvidas comuns
- [Solução de problemas](troubleshooting.md) - corrija problemas
- [Implantar Proxy](deploy-proxy.md) - rode seu próprio proxy
- [Jogos suportados](supported-games.md) - compatibilidade de jogos
