# Instalar o LightSpeed no Windows

> [!WARNING]
> Tradução assistida por máquina, não revisada por um falante nativo. A [versão em inglês](install-windows.md) é a autoritativa.

O Windows é a plataforma com foco na GUI. O pacote `lightspeed-gui` é um app independente que já contém o motor do cliente e o driver WinDivert, então você nunca precisa baixar o cliente separadamente.

---

## O que baixar

Pegue a versão mais recente na [página de Releases](https://github.com/ShibbityShwab/lightspeed/releases/latest). Para Windows x86_64 (todos os PCs Intel e AMD atuais), escolha uma das opções:

| Arquivo | Use quando |
|------|-------------|
| `lightspeed-gui-...-windows-msvc.msi` | Recomendado. Instala em Program Files, adiciona um atalho no Menu Iniciar e registra um desinstalador. |
| `lightspeed-gui-...-windows-msvc.zip` | Portátil. Descompacte em qualquer lugar e rode `lightspeed-gui.exe`. |

Os dois contêm a mesma GUI, além de `WinDivert.dll` e `WinDivert64.sys`.

> **Windows em ARM64 ainda não é um target publicado.** Os builds de release apontam para `x86_64-pc-windows-msvc`. Em um dispositivo ARM64, o build x86_64 roda sob a camada de emulação do Windows, mas não foi testado em hardware ARM64 real.

### CLI do Windows (sem suporte)

Um build de linha de comando para Windows também é publicado como zip (`lightspeed-client-...-windows-msvc.zip`). Ele existe para scripts e uso sem interface gráfica, mas é **sem suporte**: a GUI é o caminho recomendado no Windows. Se você usá-lo, rode a partir de um terminal elevado (veja abaixo).

---

## Instalação (MSI)

1. Baixe o `.msi`.
2. Dê um duplo clique e siga o assistente. O Windows SmartScreen pode avisar sobre um editor desconhecido; a release tem atestação Sigstore, então você pode verificar a atestação se quiser.
3. Abra o **LightSpeed** pelo Menu Iniciar.

## Instalação (zip portátil)

1. Baixe o `.zip`.
2. Clique com o botão direito nele, escolha **Extract All** e extraia para uma pasta em que você possa gravar (por exemplo `C:\LightSpeed`). Não rode de dentro do zip.
3. Rode `lightspeed-gui.exe`.

---

## Privilégios de Administrador

O LightSpeed usa o driver WinDivert para interceptar o tráfego UDP do jogo. O WinDivert exige direitos de **Administrador**.

- A GUI pede elevação quando precisa iniciar o interceptor. Aceite o prompt do UAC.
- Se você rodar o build da CLI, abra-o a partir de um terminal **Administrador** (clique com o botão direito no Windows Terminal ou no PowerShell e então em **Run as administrator**).

Sem elevação, o interceptor não consegue se acoplar e você verá um erro de interceptor na tela de status.

---

## Primeira execução

1. Abra a GUI e aceite o prompt do UAC.
2. A GUI descobre os relays da comunidade automaticamente pelo registro assinado. Nenhum endereço de proxy é necessário.
3. Escolha seu jogo na lista de jogos.
4. Inicie o interceptor, abra seu jogo e conecte-se a um servidor.

A GUI mostra o status dos relays, o estado do registro e os contadores de pacotes, para você ver o tráfego fluindo.

---

## Verifique se funciona

**Confira a descoberta e o registro de relays.** Abra a tela de status da GUI. Você deve ver os relays da comunidade listados (Los Angeles, New Jersey, Singapore, Frankfurt, Tokyo, Mumbai, Madrid, Sydney) com status saudável, e uma linha de registro mostrando que o handshake QUIC/auth deu certo. Se você estiver usando o build da CLI, rode:

```powershell
lightspeed-client.exe --probe-proxies
```

Isso faz uma passagem de descoberta/sondagem e imprime um relatório visível com cada relay descoberto e sua latência. Você deve ver todos os oito relays da comunidade.

**Confira o registro no control plane.** Com o build da CLI:

```powershell
lightspeed-client.exe --test-control
```

Isso se conecta ao control plane QUIC, registra uma sessão, faz ping e desconecta, imprimindo o resultado de cada passo. Um registro bem-sucedido prova que o control plane está acessível e que a autenticação está funcionando.

**Confira o fluxo de pacotes.** Na GUI, os contadores de pacotes devem subir quando seu jogo estiver conectado a um servidor. Se "Packets Sent" continuar em 0, o interceptor ainda não viu tráfego do jogo; veja [Solução de problemas](troubleshooting.md).

---

## Logs

A GUI grava um log de trace em:

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

Cole este caminho na barra de endereços do Explorador de Arquivos para abri-lo. Anexe-o a um relatório de bug se algo der errado. Veja [Solução de problemas](troubleshooting.md) para saber o que as linhas do log significam.

---

## Desinstalação

- **MSI:** Configurações → Apps → Apps instalados → LightSpeed → Desinstalar.
- **Zip:** apague a pasta extraída. O arquivo de log em `%LOCALAPPDATA%\Lightspeed\` fica para trás; apague-o manualmente se quiser uma remoção limpa.

---

## Próximos passos

- [Jogos suportados](supported-games.md)
- [Solução de problemas](troubleshooting.md)
- [Referência da CLI](CLI-REFERENCE.md)
