# 문제 해결

> [!WARNING]
> 기계 번역이며 원어민의 검수를 받지 않았습니다. [영어 원문](troubleshooting.md)이 기준입니다.

---

## 빠른 진단

먼저 내장 환경 검사를 실행하세요. 대부분의 문제를 잡아냅니다.

```bash
lightspeed --check
```

이 검사는 인터셉터 사용 가능 여부, 패킷 필터링 도구, 게임 프로필 해석, 프록시 연결성을 확인합니다.

---

## 흔한 문제

### "Interceptor not available"

**CLI:** OS에 필요한 패킷 필터링 도구가 없거나 권한이 부족합니다.

| OS | 필요 항목 | 해결 방법 |
|----|----------|------------|
| Linux | nftables 또는 iptables + root | `sudo lightspeed ...` |
| macOS | pfctl(내장) + root | `sudo lightspeed ...` |
| Windows | WinDivert 드라이버 + 관리자 | 마우스 오른쪽 버튼 → 관리자 권한으로 실행 |

다음으로 확인하세요.
```bash
lightspeed --check
```

### "No game traffic seen" / Packets Sent가 0에 머무름

**CLI:** 인터셉터가 예상 포트 범위에서 게임 패킷을 찾지 못합니다.

1. 게임이 **서버에 접속되어 있는지** 확인하세요(메인 메뉴나 로비가 아니라)
2. `--scan-processes`로 게임을 확인하세요.
   ```bash
   lightspeed --scan-processes
   ```
3. 다른 게임 프로필을 시도하거나 수동 서버 모드를 쓰세요

**GUI(Windows):** 15초 기다리세요. 노란색 "⚠ No game traffic seen" 배너가 뜨면 다음을 하세요.
1. 관리자 권한 PowerShell을 여세요.
   ```powershell
   Get-NetUDPEndpoint -OwningProcess (Get-Process RustClient).Id |
     Where-Object LocalPort -gt 1024 |
     Select-Object LocalPort
   ```
2. 표시된 포트를 **고급 → 서버 수동 설정**에 넣으세요

### "🎯 Finding your game server…"가 끝나지 않음

감지기가 1.5초 안에 같은 목적지로 가는 패킷 3개를 보지 못했습니다.

1. 게임 서버에 접속되어 있는지 확인하세요(트래픽을 만들려면 캐릭터를 움직이세요)
2. 15초가 지나도 패킷이 감지되지 않으면 서버가 비표준 포트를 쓰는 것입니다. 수동 서버 모드를 쓰세요
3. 서버에 접속한 뒤 인터셉터를 멈췄다 다시 시작하세요

### Packets Sent는 올라가는데 Packets Delivered = 0

패킷은 프록시에 도달하지만 응답이 게임에 도달하지 않습니다. 보통 방화벽 문제입니다.

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

### 프록시 상태 확인 실패

```bash
# Test connectivity
curl http://YOUR_PROXY_IP:8080/health

# Expected response:
# {"status":"ok","node_id":"relay-1","uptime_secs":86400,...}
```

연결할 수 없다면,
- 프록시가 실행 중인지 확인하세요: `systemctl status lightspeed-proxy`
- 방화벽이 UDP 4434와 TCP 8080을 허용하는지 확인하세요
- 프록시 로그를 확인하세요: `journalctl -u lightspeed-proxy --tail 50`

### 인터셉터를 시작하면 게임 연결이 끊김

인터셉터가 게임이 응답을 받기 전에 패킷을 붙잡고, 주입 경로가 실패합니다.

**Windows:**
1. `WinDivert64.sys`와 `WinDivert.dll`이 `.exe` 옆에 있는지 확인하세요
2. 보조 네트워크 어댑터(Docker, VMware, Hamachi 가상 어댑터)의 연결을 끊으세요
3. 인터셉터를 시작하기 **전에** 게임 서버에 접속하세요

**Linux:**
1. nftables 규칙을 확인하세요: `sudo nft list ruleset | grep lightspeed`
2. 규칙이 오래됐다면 `sudo lightspeed --check`로 진단하세요

### Windows에서 "WinDivert open failed" / `FWP_E_IN_USE`(0x8032000A)

WinDivert는 열린 핸들마다 WFP 콜아웃/필터를 등록합니다. 핸들이 닫히지 않으면 그 필터 상태가 남아, 눈에 보이는 프로세스가 WinDivert를 쓰지 않는데도 다음 `WinDivertOpen`이 `FWP_E_IN_USE`(0x8032000A)로 실패합니다.

최근 LightSpeed 빌드는 모든 정상 종료 경로에서 캡처 핸들과 주입 핸들을 모두 닫습니다. `--watch`와 `--start-interceptor`의 Ctrl+C, GUI의 **종료**가 포함됩니다. 또한 종료가 확정적이도록 닫기 전에 `WinDivertShutdown`으로 수신 루프를 풉니다. 강제 종료(`taskkill /f`, 크래시, 콘솔 창 닫기)는 여전히 WinDivert 2.2.x 드라이버에 오래된 상태를 남길 수 있습니다. 이는 프로세스가 사라진 뒤에는 사용자 공간에서 지울 수 없는 상위 드라이버의 한계입니다(basil00/WinDivert#294, #406).

그래도 이 문제를 겪는다면,

1. **정상 종료하고 잠시 기다리세요**: CLI에서는 Ctrl+C를, GUI에서는 **종료**를 쓴 뒤, 다시 실행하기 전에 핸들이 닫히도록 1-2초 기다리세요.
2. **WinDivert 서비스를 중지하세요**(경우에 따라 재부팅을 피할 수 있습니다. 이 드라이버는 같은 기기의 다른 WinDivert 기반 앱과 공유된다는 점에 유의하세요):
   ```powershell
   sc stop windivert
   ```
3. **재시작이 아니라 완전 종료**: Windows "다시 시작"은 오래된 상태를 담은 커널 세션을 재사용할 수 있습니다. 완전한 **종료 → 전원 켜기**가 이를 지웁니다.

> **팁:** v1.2.2 이하에서는 별개의 버그(데이터 플레인 인증이 모든 패킷을 거부하던 이슈 #59)가 연결을 멈춰 사용자가 클라이언트를 반복해서 강제 종료해야 했고, 이게 대부분의 `FWP_E_IN_USE` 보고를 촉발했습니다. 그 인증 버그는 v1.2.3에서 고쳐졌습니다.

---

## Windows GUI 문제

이 항목은 Windows의 `lightspeed-gui` 앱에 해당합니다.

### 종료가 아무 일도 하지 않고 좀비 프로세스가 남음

v1.4.2 이전 버전에서는 트레이 메뉴에서 **종료**를 골라도 프로세스가 백그라운드에 남을 수 있었습니다(좀비). 창은 닫히지만 엔진은 계속 돌아, 나중에 실행할 때 이상하게 동작했습니다. v1.4.2에서 이 문제가 고쳐졌습니다. 이제 종료하면 프로세스가 깔끔하게 끝납니다.

구버전을 쓰는데 프로세스가 멈춰 있다면 직접 끝내세요.

```powershell
Get-Process lightspeed-gui -ErrorAction SilentlyContinue | Stop-Process
```

그다음 v1.4.2 이상으로 업그레이드하세요.

### 첫 인스턴스에 포커스하지 않고 두 번째 인스턴스가 열림

v1.4.2 이전 버전에서는 GUI를 두 번 실행하면 창과 엔진이 하나 더 쌓였습니다. 이제 GUI는 단일 인스턴스 가드를 둡니다. 두 번째 실행은 다른 엔진을 시작하는 대신 "LightSpeed is already running" 안내를 잠깐 보여주고 종료합니다. 그래도(진단 목적으로) 두 번째 인스턴스를 강제로 띄우려면 `--force`를 넘기거나 `LIGHTSPEED_GUI_FORCE=1`을 설정하세요.

### 릴레이가 하나도 검색되지 않음

GUI는 서명된 레지스트리를 통해 커뮤니티 릴레이를 찾아냅니다. 릴레이 목록이 계속 비어 있다면,

1. 인터넷에 연결되어 있는지, 방화벽이나 VPN이 레지스트리로 가는 아웃바운드 HTTPS를 막고 있지 않은지 확인하세요.
2. GUI 로그(아래 참고)에서 레지스트리 가져오기나 서명 검증 오류를 확인하세요.
3. CLI 빌드에서는 `lightspeed-client --probe-proxies`를 실행해 검색과 탐색 보고서를 직접 보세요. CLI도 아무것도 못 찾으면 문제는 GUI가 아니라 네트워크 쪽입니다.
4. 연결을 고친 뒤 GUI를 다시 시작하세요. 검색은 시작할 때 실행됩니다.

### GUI 로그를 찾아 여는 방법

GUI는 추적 로그를 다음 위치에 기록합니다.

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

그 경로를 파일 탐색기 주소 표시줄에 붙여 넣어 폴더를 연 뒤, 아무 텍스트 편집기로 `gui-trace.log`를 여세요. 버그 보고에 첨부하세요.

### 로그의 "Heartbeat 0 in"

`Heartbeat 0 in` 같은 줄은 엔진이 현재 구간에서 킵얼라이브 하트비트를 하나도 보내지 않았다는 뜻입니다. 실제로는 클라이언트가 아직 동작하는 컨트롤 플레인 연결을 세우지 못해 하트비트가 나가지 않았을 때 나타납니다. 흔한 원인은 이렇습니다.

- 클라이언트가 아직 릴레이에 등록되지 않았습니다(상태 화면의 등록 줄을 확인하세요).
- 고른 릴레이에 도달할 수 없습니다.
- 인터셉터가 시작되지 않아 활성 세션이 없습니다.

등록에 성공하고 하트비트가 흐르기 시작하면 카운터가 올라갑니다. 릴레이는 정상인데 0에 머물면 `lightspeed-client --test-control`로 컨트롤 플레인에 도달할 수 있는지 따져보세요.

---

## 버그 보고용 로그

디버그 로깅으로 실행해 자세한 진단을 수집하세요.

```bash
# CLI
RUST_LOG=debug lightspeed --start-interceptor --game rust --proxy YOUR_PROXY:4434 2>&1 | tee lightspeed.log

# Windows GUI
cd C:\path\to\lightspeed
lightspeed-gui.exe 2>&1 | tee lightspeed-log.txt
```

로그 파일을 [GitHub 이슈](https://github.com/ShibbityShwab/lightspeed/issues)에 첨부하세요.

---

## 그래도 해결되지 않나요?

- [FAQ](faq.md) - 자주 묻는 질문
- [GitHub Issues](https://github.com/ShibbityShwab/lightspeed/issues) - 기존 보고 검색
- OS, 게임, 로그 출력을 포함해 새 이슈를 여세요
