# LightSpeed ユーザーガイド

> LightSpeed で ping を下げるための手順を、順を追って説明します。

---

## LightSpeed の仕組み

あなたの ISP は、速度ではなくコストで最適化された経路にゲームの通信を流しています。LightSpeed はゲームの UDP パケットを横取りし、**リレー** 経由でトンネルします。リレーとは、ゲームサーバーの地域まで高速なバックボーンでつながったデータセンター上の軽量サーバーです。既定では **コミュニティリレーネットワーク**（スポンサー資金による 8 台のリレー、署名付きレジストリで自動検出、設定不要）を使います。自分でプロキシをセルフホストすることもできます。その経路が ISP の既定ルートより速ければ、ping は下がります。

```
Your PC ──→ ISP (slow path) ──→ Game Server        ❌ High ping
Your PC ──→ LightSpeed Proxy (fast backbone) ──→ Game Server   ✅ Low ping
```

---

## 前提条件

- `lightspeed` CLI ツール、または `lightspeed-gui`（Windows）。プロキシの設定は要りません。クライアントがコミュニティリレーを自動で見つけます。
- インターセプターモードを使う場合: root または管理者権限
- 任意: セルフホストを好むなら自分のプロキシノード（[プロキシのデプロイ](deploy-proxy.md) を参照）

---

## どのアプリが必要か

| 環境 | ダウンロード | 理由 |
|-----------|----------|-----|
| **Windows** | `lightspeed-gui`（MSI または ZIP） | GUI は単体アプリで、クライアントエンジンと WinDivert ドライバーがすでに入っています。CLI は **不要** です。 |
| **Linux** | `lightspeed-gui`（または `lightspeed-client`） | GUI は Linux でも動きます（システムトレイはスタブ）。CLI は上級者向けです。 |
| **macOS** | `lightspeed-client` | 検証済みの GUI はまだありません。GUI は macOS 向けにコンパイルできますが、実機では **未検証** です。 |
| **プロキシを運用する** | `lightspeed-proxy` | VPS でリレーノードを動かす場合だけ。 |

> **必要なパッケージはいつでも 1 つだけです。** Windows のプレイヤーなら `lightspeed-gui` を入手して、残りは無視してください。`lightspeed-client` は Linux の上級者と macOS のプレイヤー向け、`lightspeed-proxy` はセルフホストする人向けです。

---

## クイックスタート（CLI、全プラットフォーム）

### 1. 環境を確認する

```bash
lightspeed --check
```

OS に必要なパケットフィルタリングツール（Linux は nftables/iptables、macOS は pfctl、Windows は WinDivert）があるかを確認します。

### 2. リレーを調べる

```bash
lightspeed --probe-proxies
```

検出した各リレーまでの遅延を表示します。初回起動時はクライアントが最速のリレーを自動で選びます。上書きするときは、自分の所在地ではなく **ゲームサーバー** に近いものを選んでください。

### 3. インターセプターを起動する

```bash
# Linux/macOS (requires root)
sudo lightspeed --start-interceptor --game rust --proxy YOUR_PROXY_IP:4434

# Windows (requires Administrator)
lightspeed --start-interceptor --game rust --proxy YOUR_PROXY_IP:4434
```

### 4. ゲームを起動する

いつもどおり任意のサーバーに接続します。LightSpeed は送信パケットからゲームサーバーを自動で見つけ、数秒でトンネルを始めます。

### 5. 監視する

CLI はリアルタイムの統計を表示します。
```
⚡ OPTIMIZING - 123.45.67.89:28015
Packets Sent: 142 | Packets Returned: 139 | Packets Delivered: 139
```

---

## クイックスタート（GUI、Windows）

### 1. ダウンロード

[Releases](https://github.com/ShibbityShwab/lightspeed/releases) から最新リリースを入手します。すべてのファイルを展開し、`WinDivert64.sys` と `WinDivert.dll` を `lightspeed-gui.exe` の隣に置いてください。

### 2. 管理者として実行する

`lightspeed-gui.exe` を右クリック → **管理者として実行**。インターセプターにはカーネルレベルのアクセスが必要です（VPN ソフトと同じです）。

### 3. リレーとゲームを選ぶ

GUI はコミュニティリレーを見つけ、初回起動時に最速のものを自動で選びます。ドロップダウンからリレーを上書きし、ゲームを選びます。

### 4. **⚡ OPTIMIZE MY ROUTE** をクリックする

ステータスが「🎯 Finding your game server…」に変わります。

### 5. ゲームを起動する

任意のサーバーに接続します。LightSpeed は数秒でそれを自動検出します。

---

### macOS の GUI（未検証）

GUI は macOS 向けにコンパイルできますが、実機では **未検証** です。リリースには
素の `tar.xz` が入っています（cargo-dist 0.32 は `.app`/`.dmg` に対応していません）。きちんとした
バンドルを作るには、Mac 上で次を実行します。

```bash
cargo build --release -p lightspeed-gui
./tools/package-macos.sh 1.6.5
```

これで `LightSpeed.app` と `LightSpeed-1.6.5.dmg` ができます。アプリはアドホック署名なので、
初回起動時は右クリック → 開く が必要です（または
`xattr -dr com.apple.quarantine LightSpeed.app`）。

---

## 適切なリレーの選び方

| 自分の所在地 | ゲームサーバーの場所 | 最適なリレー地域 |
|-----------|---------------|-------------------|
| オーストラリア | 米国西海岸 | 米国西海岸（ロサンゼルス） |
| ヨーロッパ | 米国東海岸 | 米国東海岸（ニュージャージー） |
| 東南アジア | シンガポール | シンガポール |
| 南アジア | インド | ムンバイ |
| 東アジア | 日本 | 東京 |
| 南アメリカ | 米国東海岸 | 米国東海岸（ニュージャージー） |
| どこでも | 同じ地域 | ゲームサーバーに最も近いもの |

> **目安:** リレーは自分に近いものではなく、**ゲームサーバー** に近いものを選んでください。通信は PC → リレー → ゲームサーバーと流れるので、リレーからゲームサーバーまでの区間が最も重要です。

---

## 前方誤り訂正（FEC）

FEC は、再送なしで失われたパケットを復元するために、帯域の約 25% を上乗せします。

**有効にする場合:**
- パケットロスがある（小さなカクつき、ラバーバンディング）
- 断続的な干渉のある Wi-Fi を使っている

**無効にする場合:**
- 回線がすでに飽和している
- 従量制や上限付きの回線を使っている
- パケットロスが 0.1% 未満（効果なし）

```bash
# CLI: enable FEC with default block size (K=4)
lightspeed --start-interceptor --game cs2 --proxy YOUR_PROXY:4434 --fec

# Custom block size (K=8 → 12.5% overhead)
lightspeed --start-interceptor --game cs2 --proxy YOUR_PROXY:4434 --fec --fec-k 8
```

---

## 上級者向け: 手動サーバーモード

自動検出がうまくいかない場合（独自ポート、特殊なゲーム）:

```bash
# Redirect mode: game connects to localhost, LightSpeed forwards to real server
lightspeed --game rust --game-server 123.45.67.89:28015 --proxy YOUR_PROXY:4434
```

次に、ゲームが `127.0.0.1:<port>`（LightSpeed が表示するローカルポート）に接続するように設定します。

---

## セッション途中でのサーバー切り替え

LightSpeed は、あるサーバーから切断して別のサーバーに接続したことを自動で検出します。ステータスはしばらく「🎯 Finding your game server…」を表示し、新しい接続先に固定します。手動での操作は要りません。

---

## システムトレイ（Windows GUI）

- **×** をクリックするとトレイに最小化します（終了はしません）
- 稲妻アイコンをダブルクリックすると復元します
- 右クリックで接続 / 切断 / 終了のクイックメニュー

---

## 関連項目

- [CLI リファレンス](CLI-REFERENCE.md) - すべてのフラグの説明
- [FAQ](faq.md) - よくある質問
- [トラブルシューティング](troubleshooting.md) - 問題を解決する
- [プロキシのデプロイ](deploy-proxy.md) - 自分のプロキシを運用する
- [対応ゲーム](supported-games.md) - ゲームの互換性
