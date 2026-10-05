# Instalar o LightSpeed no macOS

> [!WARNING]
> Tradução assistida por máquina, não revisada por um falante nativo. A [versão em inglês](install-macos.md) é a autoritativa.

O macOS é uma plataforma que prioriza a CLI. A GUI compila para macOS, mas não foi testada em hardware real, então o cliente de linha de comando (`lightspeed-client`) é o caminho suportado.

---

## O que baixar

Pegue a versão mais recente na [página de Releases](https://github.com/ShibbityShwab/lightspeed/releases/latest). Escolha o arquivo que corresponde à CPU do seu Mac:

| Seu Mac | Target | Arquivo |
|----------|--------|------|
| Apple Silicon (M1 e posteriores) | `aarch64-apple-darwin` | `lightspeed-client-...-aarch64-apple-darwin.tar.xz` |
| Intel | `x86_64-apple-darwin` | `lightspeed-client-...-x86_64-apple-darwin.tar.xz` |

Não sabe qual você tem? Rode:

```bash
uname -m
```

`arm64` significa Apple Silicon; `x86_64` significa Intel.

---

## Instalação

O instalador em shell é o caminho mais fácil. Ele detecta sua arquitetura e instala o cliente:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ShibbityShwab/lightspeed/releases/latest/download/lightspeed-client-installer.sh | sh
```

Ou instale manualmente a partir do arquivo:

```bash
# Substitua o nome do arquivo pelo que você baixou.
tar -xf lightspeed-client-...-aarch64-apple-darwin.tar.xz
sudo mv lightspeed-client /usr/local/bin/
```

Confirme que o binário roda:

```bash
lightspeed-client --version
```

---

## Privilégios de root

O interceptor do macOS usa `pfctl` (o filtro de pacotes embutido) para redirecionar o tráfego UDP do jogo. O `pfctl` exige root, então rode o cliente com `sudo` quando iniciar o interceptor:

```bash
sudo lightspeed-client --start-interceptor --game rust
```

Descoberta, sondagem e diagnósticos (`--probe-proxies`, `--test-control`, `--check`) não precisam de root.

---

## Primeira execução

O cliente descobre os relays da comunidade automaticamente pelo registro assinado. Nenhum endereço de proxy é necessário.

```bash
# Sonda os relays da comunidade e imprime um relatório
lightspeed-client --probe-proxies

# Inicia o interceptor para o seu jogo
sudo lightspeed-client --start-interceptor --game rust
```

---

## Verifique se funciona

**Confira a descoberta de relays.** Rode:

```bash
lightspeed-client --probe-proxies
```

Isso faz uma passagem de descoberta e sondagem e imprime um relatório visível com cada relay descoberto e sua latência. Você deve ver todos os oito relays da comunidade (Los Angeles, New Jersey, Singapore, Frankfurt, Tokyo, Mumbai, Madrid, Sydney).

**Confira o registro no control plane.** Rode:

```bash
lightspeed-client --test-control
```

Isso se conecta ao control plane QUIC, registra uma sessão, faz ping e desconecta, imprimindo o resultado de cada passo. Um registro bem-sucedido prova que o control plane está acessível e que a autenticação está funcionando.

**Confira o ambiente.** Rode:

```bash
lightspeed-client --check
```

Isso informa a disponibilidade do interceptor, o status de root, a detecção de jogos e a acessibilidade do proxy.

**Confira o fluxo de pacotes.** Depois que o interceptor estiver rodando e seu jogo estiver conectado a um servidor, os contadores de pacotes do cliente devem subir. Se "Packets Sent" continuar em 0, o interceptor ainda não viu tráfego do jogo; veja [Solução de problemas](troubleshooting.md).

---

## Nota sobre o Gatekeeper

Se o macOS bloquear o binário com "cannot be opened because the developer cannot be verified", limpe o atributo de quarentena:

```bash
xattr -d com.apple.quarantine /usr/local/bin/lightspeed-client
```

Só faça isso se você baixou o binário da página oficial de Releases.

---

## Próximos passos

- [Jogos suportados](supported-games.md)
- [Solução de problemas](troubleshooting.md)
- [Referência da CLI](CLI-REFERENCE.md)
