# macOS に LightSpeed をインストールする

macOS は CLI を優先するプラットフォームです。GUI は macOS 向けにコンパイルできますが実機では未検証なので、コマンドラインクライアント（`lightspeed-client`）がサポート対象の経路です。

---

## ダウンロードするもの

[Releases ページ](https://github.com/ShibbityShwab/lightspeed/releases/latest) から最新リリースを入手します。Mac の CPU に合うアーカイブを選びます。

| お使いの Mac | ターゲット | ファイル |
|----------|--------|------|
| Apple Silicon（M1 以降） | `aarch64-apple-darwin` | `lightspeed-client-...-aarch64-apple-darwin.tar.xz` |
| Intel | `x86_64-apple-darwin` | `lightspeed-client-...-x86_64-apple-darwin.tar.xz` |

どちらか分からないときは次を実行します。

```bash
uname -m
```

`arm64` なら Apple Silicon、`x86_64` なら Intel です。

---

## インストール

シェルインストーラーが一番簡単です。アーキテクチャを検出してクライアントをインストールします。

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ShibbityShwab/lightspeed/releases/latest/download/lightspeed-client-installer.sh | sh
```

またはアーカイブから手動でインストールします。

```bash
# Replace the filename with the one you downloaded.
tar -xf lightspeed-client-...-aarch64-apple-darwin.tar.xz
sudo mv lightspeed-client /usr/local/bin/
```

バイナリが動くか確認します。

```bash
lightspeed-client --version
```

---

## root 権限

macOS のインターセプターは `pfctl`（標準のパケットフィルター）を使ってゲームの UDP 通信をリダイレクトします。`pfctl` には root が必要なので、インターセプターを起動するときは `sudo` を付けてクライアントを実行します。

```bash
sudo lightspeed-client --start-interceptor --game rust
```

検出、プローブ、診断（`--probe-proxies`、`--test-control`、`--check`）に root は要りません。

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

## Gatekeeper に関する注意

macOS が「開発元を確認できないため開けません」という理由でバイナリをブロックしたら、隔離属性を消します。

```bash
xattr -d com.apple.quarantine /usr/local/bin/lightspeed-client
```

これは公式の Releases ページからバイナリをダウンロードした場合にだけ行ってください。

---

## 次のステップ

- [対応ゲーム](supported-games.md)
- [トラブルシューティング](troubleshooting.md)
- [CLI リファレンス](CLI-REFERENCE.md)
