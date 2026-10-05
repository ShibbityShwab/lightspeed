# Linux に LightSpeed をインストールする

Linux は CLI を優先するプラットフォームです。GUI は Linux 向けにビルドできますが、ヘッドレスや上級者向けの構成ではコマンドラインクライアント（`lightspeed-client`）がサポート対象の経路です。

---

## ダウンロードするもの

[Releases ページ](https://github.com/ShibbityShwab/lightspeed/releases/latest) から最新リリースを入手します。CPU に合うアーカイブを選びます。

| お使いのマシン | ターゲット | ファイル |
|--------------|--------|------|
| x86_64（Intel/AMD） | `x86_64-unknown-linux-gnu` | `lightspeed-client-...-x86_64-unknown-linux-gnu.tar.xz` |
| ARM64（Ampere、Graviton、Raspberry Pi 4/5） | `aarch64-unknown-linux-gnu` | `lightspeed-client-...-aarch64-unknown-linux-gnu.tar.xz` |

どちらか分からないときは次を実行します。

```bash
uname -m
```

`x86_64` なら 64 ビットの Intel/AMD、`aarch64` または `arm64` なら ARM64 です。

---

## インストール

シェルインストーラーが一番簡単です。アーキテクチャを検出してクライアントをインストールします。

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ShibbityShwab/lightspeed/releases/latest/download/lightspeed-client-installer.sh | sh
```

またはアーカイブから手動でインストールします。

```bash
# Replace the filename with the one you downloaded.
tar -xf lightspeed-client-...-x86_64-unknown-linux-gnu.tar.xz
sudo mv lightspeed-client /usr/local/bin/
```

バイナリが動くか確認します。

```bash
lightspeed-client --version
```

---

## root 権限

Linux のインターセプターは `nftables`（または `iptables`）を使ってゲームの UDP 通信をリダイレクトし、これには root が必要です。インターセプターを起動するときは `sudo` を付けてクライアントを実行します。

```bash
sudo lightspeed-client --start-interceptor --game rust
```

検出、プローブ、診断（`--probe-proxies`、`--test-control`、`--check`）に root は要りません。

ディストリビューションが既定で同梱していない場合は、`nftables` が入っていることを確認してください。

```bash
# Debian/Ubuntu
sudo apt install nftables

# Fedora/RHEL
sudo dnf install nftables

# Arch
sudo pacman -S nftables
```

---

## 初回起動

クライアントは署名付きレジストリを通じてコミュニティリレーを自動で見つけます。プロキシアドレスは不要です。

```bash
# Probe the community relays and print a report
lightspeed-client --probe-proxies

# Start the interceptor for your game
sudo lightspeed-client --start-interceptor --game rust
```

---

## 動作を確認する

**リレーの検出を確認する。** 次を実行します。

```bash
lightspeed-client --probe-proxies
```

1 回の検出とプローブを行い、見つけた各リレーとその遅延を一覧にしたレポートを表示します。8 台のコミュニティリレー（ロサンゼルス、ニュージャージー、シンガポール、フランクフルト、東京、ムンバイ、マドリード、シドニー）すべてが見えるはずです。

**コントロールプレーンの登録を確認する。** 次を実行します。

```bash
lightspeed-client --test-control
```

これは QUIC コントロールプレーンに接続し、セッションを登録し、ping を送り、切断します。各手順の結果を表示します。登録が成功すれば、コントロールプレーンに到達でき、認証も動いている証拠になります。

**環境を確認する。** 次を実行します。

```bash
lightspeed-client --check
```

インターセプターの利用可否、root の状態、ゲームの検出、プロキシへの到達性を報告します。

**パケットの流れを確認する。** インターセプターが動いていてゲームがサーバーに接続されると、クライアントのパケットカウンターが増えていくはずです。「Packets Sent」が 0 のままだと、インターセプターがまだゲームの通信をとらえていません。[トラブルシューティング](troubleshooting.md) を参照してください。

---

## 次のステップ

- [対応ゲーム](supported-games.md)
- [トラブルシューティング](troubleshooting.md)
- [CLI リファレンス](CLI-REFERENCE.md)
