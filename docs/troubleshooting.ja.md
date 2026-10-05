# トラブルシューティング

---

## 簡易診断

まず組み込みの環境チェックを実行してください。ほとんどの問題はそこで分かります。

```bash
lightspeed --check
```

次の項目を確認します。インターセプターの利用可否、パケットフィルタリングツール、ゲームプロファイルの解決、プロキシへの接続性。

---

## よくある問題

### 「Interceptor not available」

**CLI:** OS に必要なパケットフィルタリングツールがないか、権限が足りていません。

| OS | 必要なもの | 解決方法 |
|----|----------|------------|
| Linux | nftables または iptables と root | `sudo lightspeed ...` |
| macOS | pfctl（標準）と root | `sudo lightspeed ...` |
| Windows | WinDivert ドライバーと管理者権限 | 右クリック → 管理者として実行 |

次で確認します。
```bash
lightspeed --check
```

### 「No game traffic seen」、または Packets Sent が 0 のまま

**CLI:** インターセプターが想定したポート範囲でゲームのパケットを見つけられません。

1. ゲームが **サーバーに接続している** ことを確認する（メインメニューやロビーだけではだめです）
2. `--scan-processes` でゲームを確認する:
   ```bash
   lightspeed --scan-processes
   ```
3. 別のゲームプロファイルを試すか、手動サーバーモードを使う

**GUI（Windows）:** 15 秒待ちます。黄色い「⚠ No game traffic seen」バナーが出たら:
1. 管理者権限の PowerShell を開きます:
   ```powershell
   Get-NetUDPEndpoint -OwningProcess (Get-Process RustClient).Id |
     Where-Object LocalPort -gt 1024 |
     Select-Object LocalPort
   ```
2. 表示されたポートを **Advanced → set server manually** で使います

### 「🎯 Finding your game server…」から先に進まない

検出器が、同じ宛先への 3 パケットを 1.5 秒以内に確認できていません。

1. ゲームサーバーに接続していることを確認する（キャラクターを動かして通信を発生させます）
2. 15 秒たってもパケットを検出しない場合、そのサーバーは標準以外のポートを使っています。手動サーバーモードを使ってください
3. サーバーに接続してから、インターセプターを停止して再起動する

### Packets Sent は増えるが Packets Delivered = 0

パケットはプロキシに届いていますが、応答がゲームに戻っていません。たいていはファイアウォールの問題です。

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

### プロキシのヘルスチェックが失敗する

```bash
# Test connectivity
curl http://YOUR_PROXY_IP:8080/health

# Expected response:
# {"status":"ok","node_id":"relay-1","uptime_secs":86400,...}
```

到達できない場合:
- プロキシが動いているか確認する: `systemctl status lightspeed-proxy`
- ファイアウォールが UDP 4434 と TCP 8080 を許可しているか確認する
- プロキシのログを確認する: `journalctl -u lightspeed-proxy --tail 50`

### インターセプターを起動するとゲームが切断される

インターセプターが、ゲームが応答を受け取る前にパケットを奪ってしまい、注入の経路が失敗しています。

**Windows:**
1. `WinDivert64.sys` と `WinDivert.dll` が `.exe` の隣にあるか確認する
2. セカンダリのネットワークアダプター（Docker、VMware、Hamachi の仮想アダプター）を切断する
3. インターセプターを起動する **前に** ゲームサーバーへ接続する

**Linux:**
1. nftables のルールを確認する: `sudo nft list ruleset | grep lightspeed`
2. ルールが古い場合: `sudo lightspeed --check` で診断する

### Windows で「WinDivert open failed」または `FWP_E_IN_USE`（0x8032000A）

WinDivert は開いたハンドルごとに WFP のコールアウトとフィルターを登録します。ハンドルが閉じられないままだとそのフィルター状態が残り、次に `WinDivertOpen` を呼ぶと、WinDivert を使っているプロセスが見当たらなくても `FWP_E_IN_USE`（0x8032000A）で失敗します。

最近の LightSpeed のビルドでは、正常な終了経路すべてでキャプチャと注入の両方のハンドルを閉じます。`--watch` や `--start-interceptor` での Ctrl+C や、GUI の **Quit** も含まれます。また、終了を確実にするため、閉じる前に `WinDivertShutdown` で受信ループを解除します。強制終了（`taskkill /f`、クラッシュ、コンソールウィンドウを閉じる）では、WinDivert 2.2.x ドライバーに古い状態が残ることがあります。これは上流ドライバーの制約（basil00/WinDivert#294、#406）で、プロセスが消えた後はユーザー空間からは解消できません。

それでも起きる場合:

1. **正常に終了して少し待つ**: CLI では Ctrl+C、GUI では **Quit** を使い、再起動する前に 1、2 秒ほどハンドルが閉じるのを待ちます。
2. **WinDivert サービスを停止する**（場合によっては再起動を避けられます。このドライバーはマシン上の他の WinDivert ベースのアプリと共有されている点に注意してください）:
   ```powershell
   sc stop windivert
   ```
3. **再起動ではなく完全なシャットダウン**: Windows の「再起動」は古い状態を抱えたカーネルセッションを再利用することがあります。**シャットダウン → 電源投入** で解消します。

> **ヒント:** v1.2.2 以前には別の不具合（データプレーンの認証がすべてのパケットを拒否する、issue #59）があり、接続が固まってユーザーがクライアントを繰り返し強制終了していました。これが `FWP_E_IN_USE` の報告のほとんどを引き起こした原因です。この認証の不具合は v1.2.3 で修正されています。

---

## Windows GUI の問題

ここからは Windows の `lightspeed-gui` アプリに当てはまる内容です。

### Quit が何もせず、ゾンビプロセスが残った

v1.4.2 より前のバージョンでは、トレイメニューから **Quit** を選んでもプロセスがバックグラウンドに残ることがあり（ゾンビ）、ウィンドウは閉じるのにエンジンは動き続け、次回の起動がおかしくなっていました。v1.4.2 で修正され、Quit はプロセスをきちんと終了します。

古いビルドを使っていてプロセスが固まっている場合は、手動で終了します。

```powershell
Get-Process lightspeed-gui -ErrorAction SilentlyContinue | Stop-Process
```

その後、v1.4.2 以降にアップグレードしてください。

### 既存のウィンドウにフォーカスせず、2 つ目のインスタンスが開く

v1.4.2 より前のバージョンでは、GUI を 2 回起動すると 2 つ目のウィンドウと 2 つ目のエンジンが重なって起動していました。現在の GUI は単一インスタンスのガードを備えており、2 回目の起動では「LightSpeed is already running」という短い通知を表示して終了し、別のエンジンは起動しません。診断のためにどうしても 2 つ目を起動したい場合は、`--force` を渡すか `LIGHTSPEED_GUI_FORCE=1` を設定してください。

### リレーが 1 つも見つからない

GUI は署名付きレジストリを通じてコミュニティリレーを見つけます。リレー一覧が空のままなら:

1. インターネットに接続できているか、ファイアウォールや VPN がレジストリへの外向き HTTPS を妨げていないかを確認します。
2. GUI のログ（下記参照）にレジストリ取得や署名検証のエラーがないか確認します。
3. CLI ビルドでは `lightspeed-client --probe-proxies` を実行すると、検出とプローブのレポートを直接確認できます。CLI でも何も見つからないなら、問題は GUI 固有ではなくネットワーク側です。
4. 接続性を直した後に GUI を再起動します。検出は起動時に走ります。

### GUI のログを見つけて開く方法

GUI はトレースログを次に書き出します。

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

そのパスをエクスプローラーのアドレスバーに貼り付けるとフォルダーが開くので、`gui-trace.log` を任意のテキストエディターで開きます。不具合報告に添付してください。

### ログの「Heartbeat 0 in」

`Heartbeat 0 in` のような行は、現在の期間中にエンジンがキープアライブのハートビートを 1 つも送っていないことを意味します。実際には、クライアントがまだコントロールプレーンへの接続を確立しておらず、ハートビートが出ていないときに現れます。よくある原因:

- クライアントがまだリレーに登録していない（ステータス画面の登録の行を確認してください）。
- 選択したリレーに到達できない。
- インターセプターが起動しておらず、セッションが有効でない。

登録が成功してハートビートが流れ始めればカウンターは増えます。リレーが正常なのに 0 のままなら、`lightspeed-client --test-control` を実行して、コントロールプレーンに到達できるかを切り分けてください。

---

## 不具合報告用のログ

デバッグログを付けて実行すると、詳しい診断情報を取得できます。

```bash
# CLI
RUST_LOG=debug lightspeed --start-interceptor --game rust --proxy YOUR_PROXY:4434 2>&1 | tee lightspeed.log

# Windows GUI
cd C:\path\to\lightspeed
lightspeed-gui.exe 2>&1 | tee lightspeed-log.txt
```

ログファイルは [GitHub の issue](https://github.com/ShibbityShwab/lightspeed/issues) に添付してください。

---

## それでも解決しない場合

- [FAQ](faq.md) - よくある質問
- [GitHub Issues](https://github.com/ShibbityShwab/lightspeed/issues) - 既存の報告を検索する
- OS、ゲーム、ログ出力を添えて新しい issue を作成する
