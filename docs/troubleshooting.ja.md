# トラブルシューティング

> [!WARNING]
> 機械翻訳によるもので、ネイティブによる確認は行われていません。正式な内容は[英語版](troubleshooting.md)を参照してください。

---

## クイック診断

最初に組み込み環境チェックを実行します。これにより、ほとんどの問題が検出されます。

```bash
lightspeed --check
```

これにより、インターセプタの可用性、パケット フィルタリング ツール、ゲーム プロファイルの解決、およびプロキシ接続が検証されます。

---

## よくある問題

### 「インターセプターが利用できません」

**CLI:** OS に必要なパケット フィルタリング ツールがないか、権限がありません。

| OS |必須 |修正方法 |
|------|----------|---------------|
|リナックス | nftables または iptables + root | `sudo lightspeed ...` |
| macOS | pfctl (組み込み) + root | `sudo lightspeed ...` |
|ウィンドウズ | WinDivert ドライバー + 管理者 |右クリック → 管理者として実行 |

次の方法で確認します。
```bash
lightspeed --check
```

### 「ゲーム トラフィックが見られません」/ 送信パケット数は 0 のままです

**CLI:** インターセプターは、予期されたポート範囲でゲーム パケットを見つけることができません。

1. ゲームが**サーバー** (メインメニューやロビーだけでなく) に接続されていることを確認してください。
2. `--scan-processes` でゲームを確認します。
   ```bash
   lightspeed --scan-processes
   ```
3. 別のゲームプロファイルを試すか、手動サーバーモードを使用してください

**GUI (Windows):** 15 秒待ちます。黄色の「⚠ ゲーム トラフィックが見られません」というバナーが表示された場合:
1. 管理者特権の PowerShell を開きます。
   ```powershell
   Get-NetUDPEndpoint -OwningProcess (Get-Process RustClient).Id |
     Where-Object LocalPort -gt 1024 |
     Select-Object LocalPort
   ```
2. **[詳細設定] → [サーバーを手動で設定]** に示されているポートを使用します。

### 「🎯 ゲームサーバーを探しています…」が解決しない

検出器は、1.5 秒以内に同じ宛先への 3 つのパケットを検出していません。

1. ゲームサーバーに接続していることを確認します (キャラクターを移動してトラフィックを生成します)
2. 15 秒経ってもパケットが検出されない場合、サーバーは非標準ポート上にあります。手動サーバー モードを使用してください。
3. サーバーに接続した後、インターセプターを停止して再起動します。

### 送信パケット数が増加、配信パケット数 = 0

パケットはプロキシに到達しますが、応答はゲームに到達しません。通常はファイアウォールの問題です。

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
# 安定したルールが存在するか確認します (ブーストごとに1つではなく、単一のルールです)
netsh advfirewall firewall show rule name="LightSpeed WinDivert Tunnel"

# 存在しない場合は手動で追加します
netsh advfirewall firewall add rule name="LightSpeed WinDivert Tunnel" protocol=UDP dir=in action=allow program="C:\path\to\lightspeed-gui.exe"
```

以前のビルドのルールは `LightSpeed WinDivert Tunnel <port>` という名前でした。現在のビルドは使用するポートのルールを削除し、単一の安定したルールを保持します。残っているポート別ルールは削除して問題ありません:
`netsh advfirewall firewall delete rule name="LightSpeed WinDivert Tunnel 4434"`
(`netsh advfirewall firewall show rule name=all` に表示されるポートに合わせてください)。

### `sendmsg error: ... 10040` / `WSAEMSGSIZE` (Windows) または `EMSGSIZE` (Linux/macOS)

クライアントが送信しようとしたデータグラムが、**実際の**パス MTU より大きい状態です。トンネルの既定のクランプは 1492 バイト (PPPoE 対応) ですが、VPN、Zapret のような DPI シム、MTU を下げたアダプタではそれより小さいことがあるため、クランプ内に収まるパケットでも OS に拒否されます (Windows エラー 10040)。

修正 — `config.toml` でトンネルのパス MTU 上限を下げます:

```toml
[tunnel]
# 1280-1500。まず 1400、エラーが続く場合は 1350 にしてください。
mtu = 1400
```

または CLI で `--mtu 1400` を渡します。現在のビルドは、送信が拒否されると予算を自動的に下げ、そのデータグラムをフラグメンテーション許可で一度だけ再送し、リレーが無応答であるかのように扱う代わりに OS エラーをステータスに記録します。

クライアントが更新されるまでの暫定回避策: Windows アダプタの MTU を下げてゲームがより小さいパケットを送るようにし、ゲームを再起動します:

```powershell
netsh interface ipv4 show subinterfaces
netsh interface ipv4 set subinterface "Ethernet" mtu=1400 store=persistent
```

### プロキシのヘルスチェックが失敗する

```bash
# Test connectivity
curl http://YOUR_PROXY_IP:8080/health

# Expected response:
# {"status":"ok","node_id":"relay-1","uptime_secs":86400,...}
```

到達できない場合:
- プロキシが実行されていることを確認します: `systemctl status lightspeed-proxy`
- ファイアウォールで UDP 4434 および TCP 8080 が許可されていることを確認してください
- プロキシ ログを確認します: `journalctl -u lightspeed-proxy --tail 50`

### QUIC コントロール プレーンが `error 120` / `no_application_protocol` で失敗します

クライアントはリレーとの QUIC コントロール プレーン ハンドシェイクを完了できませんでした。とき
ワンショットの空のオファーの再試行も失敗し、ログはこの判定で終わります (引用符付き)
クライアントからの言葉どおり。 `{err}` は基礎となる QUIC エラー、`{addr}` は
応答者のアドレス):

```text
{err}: the responder at {addr} closed the QUIC handshake with TLS alert 120 (no_application_protocol), and the empty-offer retry also failed. The client cannot tell these two causes apart from the handshake alone: (1) a QUIC-intercepting middlebox on this path (VPN/AV/ISP) that does not carry the ALPN extension (see #140); or (2) a LightSpeed relay on a different ALPN revision -- version skew, where the relay requires an ALPN name this client does not offer.
```

**クライアントがすでに行ったこと。** 現在のクライアントは常にバージョン付きの ALPN 名を提供します。
`lightspeed-control/1`。リレーが TLS アラート 120 で応答すると、クライアントは再試行します。
**1 回** 空の ALPN オファーを使用し、残りはそのリレーの回答を記憶します
したがって、通常のクライアント操作では、運命の余分なハンドシェイクが次の時点で発生します。
発射ごとにリレーごとに最大 1 回。次のリレーに対してフォールバックが成功するのは正常です。
アップグレードされていません (リレーのデフォルトは `[network] control_alpn = "off"`):
`QUIC connection established`行に`alpn=none`が表示され、登録が進みます。

**最後の失敗の意味** 空のオファーの再試行も失敗したため、
まさに 2 つの可能性があり、ハンドシェイクだけではそれらを区別することはできません。

1. **パス上の QUIC インターセプトミドルボックス** - VPN、ウイルス対策/ファイアウォール
   HTTPS/QUIC インスペクション、または ALPN 拡張子を持たない ISP 機器
   ([#140](https://github.com/ShibbityShwab/lightspeed/issues/140))。リレーごとなら
   あるネットワークでは障害が発生しても、同じリレーが別のネットワークでは動作し、パスは共通です
   要因。
2. **バージョン スキュー** - リレーには、このクライアントが提供していない ALPN リビジョンが必要です
   (たとえば、リレーが `control_alpn = "required"` に設定され、プロトコルを実行している場合)
   このクライアントよりも新しいリビジョン）。これは設計上、静かに失敗することはなく、大声で失敗します。
   ダウングレード: `lightspeed-control/1` のリレーに対する現在のクライアントがネゴシエートします。
   そして、このクライアントが提供しないものを要求するリレーは、それを 120 で拒否します。

**絞り込む方法:**

- `lightspeed-client --test-control --proxy <relay-ip>` を実行して、どの段階が該当するかを確認します。
  リレーはあります。接続されたラインと最終的なサマリーの `alpn=` 値は、
  ネゴシエートされたアプリケーション プロトコル: `lightspeed-control/1` はリレーが受け入れたことを意味します
  バージョン管理されたオファー `none` は、フォールバック後に空のオファーを受け入れたことを意味します (古い
  リレー）、このセクションの判定で失敗した実行は上記のケースです。
- このネットワークではすべてのリレーが失敗するが、他の場所では同じリレーが動作する場合は、
  ミドルボックス: VPN またはセキュリティ製品の HTTPS/QUIC インスペクションを無効にして再試行します。
  次に、UDP 4433 キャプチャを #140 に接続します。
- 1 つのリレーのみが失敗する場合は、そのリレーの ALPN モードをクライアント ビルドと比較します。
  リレーの開始行とその `GET /health` の `control_alpn` フィールドは、
  構成済みモード ([CLI リファレンス](CLI-REFERENCE.md#proxy-configuration) を参照)。もしあなたが
  そのリレーを操作すると、`control_alpn = "off"` は古いものに対して以前の動作を復元します。
  クライアント。演算子プロシージャとクライアントファーストの順序付けルールは次のとおりです。
  [プロキシのデプロイ](deploy-proxy.md) と [ALPN ロールアウト計画](quic-alpn-rollout.md)。

### インターセプターの開始時にゲームが切断される

ゲームが応答を受信する前にインターセプターがパケットを捕捉し、インジェクト パスが失敗します。

**Windows:**
1. `WinDivert64.sys` と `WinDivert.dll` が `.exe` の隣にあることを確認します。
2. セカンダリ ネットワーク アダプタ (Docker、VMware、Hamachi 仮想アダプタ) を切断します。
3. インターセプターを開始する **前** ゲームサーバーに接続します

**Linux:**
1. nftables ルールを確認します: `sudo nft list ruleset | grep lightspeed`
2. ルールが古い場合: `sudo lightspeed --check` を診断します

### 「WinDivert オープンに失敗しました」/ `FWP_E_IN_USE` (0x8032000A) (Windows 上)

WinDivert は、開いているハンドルごとに WFP コールアウト/フィルターを登録します。ハンドルが閉じられない場合、そのフィルター状態は残り、WinDivert を使用しているプロセスが表示されていない場合でも、次の `WinDivertOpen` は `FWP_E_IN_USE` (0x8032000A) で失敗します。

最近の LightSpeed ビルドでは、`--watch` と `--start-interceptor` の Ctrl+C および GUI の **Quit** を含む、すべての正常なシャットダウン パス上のキャプチャ ハンドルとインジェクト ハンドルの両方が閉じられ、閉じる前に `WinDivertShutdown` で受信ループのブロックが解除されるため、ティアダウンは決定的です。ハードキル (`taskkill /f`、クラッシュ、またはコンソール ウィンドウを閉じる) を実行しても、WinDivert 2.2.x ドライバーが古い状態のままになる可能性があります。これは、プロセスが終了するとユーザー空間でクリアできないアップストリーム ドライバーの制限 (basil00/WinDivert#294、#406) です。

それでもヒットする場合:

1. **正常に終了してしばらく待ちます**: CLI Ctrl+C または GUI の **Quit** を使用し、再起動する前に 1 ～ 2 秒待ってハンドルを閉じます。
2. **WinDivert サービスを停止します** (場合によっては再起動が不要になります。ドライバーはマシン上の他の WinDivert ベースのアプリと共有されていることに注意してください)。
   ```powershell
   sc stop windivert
   ```
3. **再起動ではなく完全なシャットダウン**: Windows の「再起動」により、古い状態を保持しているカーネル セッションを再利用できます。完全な **シャットダウン→電源オン** でクリアされます。

> **ヒント:** v1.2.2 以前では、別のバグ (すべてのパケットを拒否するデータプレーン認証、問題 #59) により、接続がフリーズし、ユーザーはクライアントを繰り返し強制終了する必要がありました。これが、ほとんどの `FWP_E_IN_USE` レポートの原因となっていました。この認証バグは v1.2.3 で修正されています。

---

## Windows GUI の問題

これらは Windows 上の `lightspeed-gui` アプリに適用されます。

### Quit は何も行わず、ゾンビプロセスを残しました

v1.4.2 より前のバージョンでは、トレイ メニューから **Quit** を選択すると、プロセスがバックグラウンド (ゾンビ) で実行されたままになる可能性があり、そのためウィンドウは閉じてもエンジンは実行され続け、その後の起動が奇妙に動作することがありました。 v1.4.2 ではこの問題が修正されています。「今すぐ終了」を選択すると、プロセスが正常に終了します。

古いビルドを使用していてプロセスが停止している場合は、手動で終了します。

```powershell
Get-Process lightspeed-gui -ErrorAction SilentlyContinue | Stop-Process
```

次に、v1.4.2 以降にアップグレードします。

### 最初のインスタンスにフォーカスする代わりに、2 番目のインスタンスが開きます。

v1.4.2 より前のバージョンでは、GUI を 2 回起動すると、2 番目のウィンドウと 2 番目のエンジンがスタックされました。 GUI は単一インスタンス ガードを保持するようになりました。2 回目の起動では、「LightSpeed はすでに実行されています」という短い通知が表示され、別のエンジンを開始する代わりに終了します。とにかく 2 番目のインスタンスを強制するには (診断用)、`--force` を渡すか、`LIGHTSPEED_GUI_FORCE=1` を設定します。

### リレーが検出されませんでした

GUI は、署名されたレジストリを通じてコミュニティ リレーを検出します。リレーリストが空のままの場合:

1. インターネットにアクセスでき、ファイアウォールまたは VPN がレジストリへのアウトバウンド HTTPS をブロックしていないことを確認します。
2. GUI ログ (以下を参照) でレジストリのフェッチまたは署名検証エラーを確認します。
3. CLI ビルドで `lightspeed-client --probe-proxies` を実行して、検出レポートとプローブ レポートを直接確認します。 CLI でも何も見つからなかった場合、問題は GUI 固有ではなく、ネットワーク側にあります。
4. 接続を修正した後、GUI を再起動します。検出は起動時に実行されます。

### GUI ログを見つけて開く方法

GUI はトレース ログを次の場所に書き込みます。

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

そのパスをファイル エクスプローラーのアドレス バーに貼り付けてフォルダーを開き、任意のテキスト エディターで `gui-trace.log` を開きます。バグレポートに添付してください。

### ログ内の「ハートビート 0 インチ」

`Heartbeat 0 in` のような行は、エンジンが現在のウィンドウでキープアライブ ハートビートを送信しなかったことを意味します。実際には、クライアントが機能するコントロール プレーン接続をまだ確立していないため、ハートビートが発生していないときに発生します。一般的な原因:

- クライアントはまだリレーに登録されていません (ステータス ビューの登録行を確認してください)。
- 選択したリレーに到達できません。
- インターセプターが開始されていないため、アクティブなセッションはありません。

登録が成功し、ハートビートが流れ始めると、カウンターが上昇します。リレーが正常であるにもかかわらず 0 のままの場合は、`lightspeed-client --test-control` を実行して、コントロール プレーンに到達可能かどうかを確認します。

---

## バグレポートのログ

デバッグ ログを使用して実行して、詳細な診断を取得します。

```bash
# CLI
RUST_LOG=debug lightspeed --start-interceptor --game rust --proxy YOUR_PROXY:4434 2>&1 | tee lightspeed.log

# Windows GUI
cd C:\path\to\lightspeed
lightspeed-gui.exe 2>&1 | tee lightspeed-log.txt
```

ログ ファイルを [GitHub の問題](https://github.com/ShibbityShwab/lightspeed/issues) に添付します。

---

## まだ行き詰まっていますか?

- [FAQ](faq.md) - よくある質問
- [GitHub の問題](https://github.com/ShibbityShwab/lightspeed/issues) - 既存のレポートを検索
- OS、ゲーム、ログ出力に関する新しい問題を開く