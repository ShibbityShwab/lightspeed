# 문제 해결

> [!WARNING]
> 기계 번역이며 원어민의 검수를 받지 않았습니다. [영어 원문](troubleshooting.md)이 기준입니다.

---

## 빠른 진단

내장 환경 검사를 먼저 실행하세요. 그러면 대부분의 문제가 발견됩니다.

```bash
lightspeed --check
```

이는 인터셉터 가용성, 패킷 필터링 도구, 게임 프로필 확인 및 프록시 연결을 확인합니다.

---

## 일반적인 문제

### "인터셉터를 사용할 수 없습니다."

**CLI:** 귀하의 OS에 필수 패킷 필터링 도구가 없거나 권한이 부족합니다.

| OS | 필수 | 수정 방법 |
|----|----------|------------|
| 리눅스 | nftables 또는 iptables + 루트 | `sudo lightspeed ...` |
| macOS | pfctl(내장) + 루트 | `sudo lightspeed ...` |
| 윈도우 | WinDivert 드라이버 + 관리자 | 마우스 오른쪽 버튼 클릭 → 관리자 권한으로 실행 |

다음을 통해 확인하세요.
```bash
lightspeed --check
```

### "게임 트래픽이 표시되지 않음" / 보낸 패킷이 0으로 유지됨

**CLI:** 인터셉터가 예상 포트 범위에서 게임 패킷을 찾을 수 없습니다.

1. 게임이 **서버에 연결**되어 있는지 확인하세요(메인 메뉴나 로비뿐만 아니라).
2. `--scan-processes`로 게임을 확인합니다.
   ```bash
   lightspeed --scan-processes
   ```
3. 다른 게임 프로필을 사용하거나 수동 서버 모드를 사용하세요.

**GUI(Windows):** 15초 동안 기다립니다. 황색 "⚠ 게임 트래픽이 확인되지 않음" 배너가 나타나는 경우:
1. 관리자 권한 PowerShell을 엽니다.
   ```powershell
   Get-NetUDPEndpoint -OwningProcess (Get-Process RustClient).Id |
     Where-Object LocalPort -gt 1024 |
     Select-Object LocalPort
   ```
2. **고급 → 수동 서버 설정**에 표시된 포트를 사용하세요.

### "🎯 게임 서버를 찾는 중…"이 해결되지 않음

탐지기는 1.5초 이내에 동일한 대상으로 향하는 3개의 패킷을 감지하지 못했습니다.

1. 게임 서버에 연결되어 있는지 확인하세요. (캐릭터를 움직여 트래픽을 생성하세요.)
2. 15초 후에도 여전히 패킷이 감지되지 않으면 서버가 비표준 포트에 있는 것입니다. 수동 서버 모드를 사용하세요.
3. 서버 접속 후 인터셉터를 중지했다가 다시 시작합니다.

### 보낸 패킷 증가, 전달된 패킷 = 0

패킷이 프록시에 도달하지만 응답이 게임에 도달하지 않습니다. 일반적으로 방화벽 문제입니다.

**리눅스:**
```bash
sudo iptables -I INPUT -p udp --sport 4434 -j ACCEPT
```

**맥OS:**
```bash
sudo pfctl -d  # Temporarily disable pf to test
```

**윈도우:**
```powershell
# Check if the stable rule exists (one rule, not one per boost)
netsh advfirewall firewall show rule name="LightSpeed WinDivert Tunnel"

# Add it manually if missing
netsh advfirewall firewall add rule name="LightSpeed WinDivert Tunnel" protocol=UDP dir=in action=allow program="C:\path\to\lightspeed-gui.exe"
```

이전 빌드의 규칙 이름은 `LightSpeed WinDivert Tunnel <port>`였습니다. 현재
빌드에서는 사용하는 포트에 대한 포트를 제거하고 안정적인 단일 규칙을 유지합니다. 모두
나머지 포트별 규칙은 삭제해도 안전합니다.
`netsh advfirewall firewall delete rule name="LightSpeed WinDivert Tunnel 4434"`
(`netsh advfirewall firewall show rule name=all`에 표시된 포트와 일치).

### `sendmsg error: ... 10040` / `WSAEMSGSIZE`(Windows) 또는 `EMSGSIZE`(Linux/macOS)

클라이언트가 전송하려고 시도한 데이터그램이 **실제** 경로 MTU보다 큽니다. 는
기본 터널 클램프는 1492바이트(PPPoE 안전)이지만 VPN, DPI 심(예:
Zapret 또는 감소된 MTU 어댑터는 더 작을 수 있으므로 클램프에 맞는 패킷
OS(`Windows error 10040`)에서는 여전히 거부됩니다.

수정 — `config.toml`에서 터널의 경로 MTU 한도를 낮춥니다.

```toml
[tunnel]
# 1280-1500. Start at 1400, then 1350 if the error persists.
mtu = 1400
```

또는 CLI에서 `--mtu 1400`를 전달하세요. 현재 빌드도 예산을 낮춥니다.
전송이 거부되면 자동으로 해당 데이터그램을 한 번 다시 시도합니다.
조각화를 허용하고 대신 상태에 OS 오류를 기록합니다.
릴레이를 무음으로 보고합니다.

클라이언트가 변경되지 않은 동안 즉시 해결 방법: Windows 어댑터의 성능을 낮추세요.
MTU를 사용하면 게임에서 더 작은 패킷을 방출한 다음 게임을 다시 시작합니다.

```powershell
netsh interface ipv4 show subinterfaces
netsh interface ipv4 set subinterface "Ethernet" mtu=1400 store=persistent
```

### 프록시 상태 확인 실패

```bash
# Test connectivity
curl http://YOUR_PROXY_IP:8080/health

# Expected response:
# {"status":"ok","node_id":"relay-1","uptime_secs":86400,...}
```

연결할 수 없는 경우:
- 프록시가 실행 중인지 확인: `systemctl status lightspeed-proxy`
- 방화벽이 UDP 4434 및 TCP 8080을 허용하는지 확인하세요.
- 프록시 로그 확인: `journalctl -u lightspeed-proxy --tail 50`

### `error 120` / `no_application_protocol`로 인해 QUIC 제어 평면이 실패함

클라이언트가 릴레이를 사용하여 QUIC 제어 평면 핸드셰이크를 완료할 수 없습니다. 때
일회성 빈 제안 재시도도 실패했습니다. 로그는 다음 결과로 끝납니다(인용문 포함).
클라이언트에서 그대로; `{err}`는 기본 QUIC 오류이고 `{addr}`는
응답자 주소):

```text
{err}: the responder at {addr} closed the QUIC handshake with TLS alert 120 (no_application_protocol), and the empty-offer retry also failed. The client cannot tell these two causes apart from the handshake alone: (1) a QUIC-intercepting middlebox on this path (VPN/AV/ISP) that does not carry the ALPN extension (see #140); or (2) a LightSpeed relay on a different ALPN revision -- version skew, where the relay requires an ALPN name this client does not offer.
```

**클라이언트가 이미 수행한 작업.** 현재 클라이언트는 항상 버전이 지정된 ALPN 이름을 제공합니다.
`lightspeed-control/1`. 릴레이가 TLS 경고 120으로 응답하면 클라이언트는 다시 시도합니다.
빈 ALPN 제안으로 **한 번** 그런 다음 나머지에 대한 해당 릴레이의 응답을 기억합니다.
프로세스의 정상적인 클라이언트 작업에서는 운명의 추가 핸드셰이크가 발생합니다.
실행 시 릴레이당 가장 많이 한 번입니다. 성공적인 폴백은 다음과 같은 릴레이에 대해 정상입니다.
업그레이드되지 않았습니다(릴레이 기본값은 `[network] control_alpn = "off"`입니다):
`QUIC connection established` 라인에 `alpn=none`가 표시되고 등록이 진행됩니다.

**최종 실패의 의미.** 빈 제안 재시도도 실패했기 때문에
정확히 두 가지 가능성이 있으며 악수만으로는 이를 구분할 수 없습니다.

1. **경로에 있는 QUIC 가로채기 미들박스** - VPN, 바이러스 백신/방화벽
   HTTPS/QUIC 검사 또는 ALPN 확장을 지원하지 않는 ISP 장비
   ([#140](https://github.com/ShibbityShwab/lightspeed/issues/140)). 모든 릴레이의 경우
   한 네트워크에서는 실패하지만 동일한 릴레이가 다른 네트워크에서는 작동하는 경우 경로는 공통입니다.
   요인.
2. **버전 차이** - 릴레이에는 이 클라이언트가 제공하지 않는 ALPN 개정판이 필요합니다.
   (예를 들어 릴레이는 `control_alpn = "required"`로 설정되고 프로토콜을 실행합니다.
   이 클라이언트보다 최신 버전). 이것은 의도적으로 큰 소리로 실패하며 결코 자동으로 실패하지 않습니다.
   다운그레이드: `lightspeed-control/1`의 릴레이에 대해 현재 클라이언트가 협상합니다.
   이 클라이언트가 제공하지 않는 것을 요구하는 릴레이는 120으로 이를 거부합니다.

**범위를 좁히는 방법:**

- `lightspeed-client --test-control --proxy <relay-ip>`를 실행하여 어떤 단계가 해당되는지 확인하세요.
  릴레이가 있습니다. 연결된 라인과 최종 요약의 `alpn=` 값은
  협상된 애플리케이션 프로토콜: `lightspeed-control/1`는 릴레이가
  버전이 지정된 제안인 `none`는 대체 후 빈 제안(이전 버전)을 수락했음을 의미합니다.
  릴레이), 이 섹션의 판결로 실패하는 실행은 위의 경우입니다.
- 이 네트워크의 모든 릴레이가 실패하지만 다른 곳에서는 동일한 릴레이가 작동하는 경우
  미들박스: VPN 또는 보안 제품의 HTTPS/QUIC 검사를 비활성화하고 재시도합니다.
  그런 다음 UDP 4433 캡처를 #140에 연결합니다.
- 하나의 릴레이만 실패하는 경우 해당 릴레이의 ALPN 모드를 클라이언트 빌드와 비교하십시오.
  릴레이의 시작 라인과 `GET /health`의 `control_alpn` 필드는 다음을 보고합니다.
  구성된 모드([CLI 참조](CLI-REFERENCE.md#proxy-configuration) 참조). 당신이
  해당 릴레이를 작동하면 `control_alpn = "off"`가 이전 동작을 복원합니다.
  클라이언트; 연산자 절차와 클라이언트 우선 순서 규칙이 있습니다.
  [프록시 배포](deploy-proxy.md) 및 [ALPN 롤아웃 계획](quic-alpn-rollout.md).

### 인터셉터가 시작되면 게임 연결이 끊어집니다.

인터셉터는 게임이 응답을 받기 전에 패킷을 점유하고 주입 경로가 실패합니다.

**윈도우:**
1. `WinDivert64.sys` 및 `WinDivert.dll`가 `.exe` 옆에 있는지 확인합니다.
2. 보조 네트워크 어댑터(Docker, VMware, Hamachi 가상 어댑터) 연결을 끊습니다.
3. 인터셉터를 시작하기 **전에** 게임 서버에 연결하세요

**리눅스:**
1. nftables 규칙 확인: `sudo nft list ruleset | grep lightspeed`
2. 규칙이 오래된 경우: 진단용 `sudo lightspeed --check`

### Windows에서 "WinDivert 열기 실패" / `FWP_E_IN_USE`(0x8032000A)

실제 캡처 필터를 가진 핸들, 즉 intercept(`udp and outbound …`)와 shadow(`udp and inbound …`) 핸들만 WFP 콜아웃/필터를 소유합니다. 앱 수명 동안 유지되는 pin 핸들과 inject 핸들은 `"false"` 필터를 사용하고 트래픽을 우회시키지 않으며 WFP 객체를 소유하지 않습니다. 따라서 WFP 콜아웃/필터는 '열린 핸들마다 하나'가 아니며, 유지 중인 pin은 WFP 상태 잠금이 아닙니다. 2026-11-23 필드 추적에서는 pin이 열려 있는데도 다음 intercept 열기가 여전히 `FWP_E_IN_USE`로 실패했습니다.

`FWP_E_IN_USE`는 WFP 객체가 아직 참조되고 있거나(예: 강제 종료 후 커널이 이전 프로세스의 필터를 아직 삭제하지 못함) 콜아웃이 동시에 등록/해제 중임을 뜻합니다. 'WinDivert를 사용하는 프로세스가 보이지 않는다'는 참조가 없다는 증거가 아니며, 현재 WFP 상태를 캡처하지 않으므로 오류 문구를 근본 원인으로 취급하지 마세요.

LightSpeed는 모든 정상 종료 경로(`--watch`/`--start-interceptor`의 Ctrl+C, GUI의 **종료**)에서 intercept, inject, shadow 핸들을 닫고, 닫기 전에 `WinDivertShutdown`으로 수신 루프를 풀며, 소유 스레드의 확인을 기다립니다. 이 결정성은 앱의 **핸들 소유권**에 대한 것이며, 커널 측 WFP teardown은 별도 경로이고 그 실패는 사용자 공간에서 보이지 않습니다.

그래도 발생한다면:

1. **Start를 한 번 다시 시도하세요.** 다음 패치에서는 pin이 유지된 상태에서도 `FWP_E_IN_USE`를 자동으로 재시도하며(상한 있음, 약 5초), pin을 한 번 재순환할 수 있습니다. 1.12.5는 **pin이 없을 때만** 재시도하므로, 그 버전에서는 Stop 후 다시 Optimize해야 합니다.
2. **정상적으로 종료하고 잠시 기다리세요**: CLI의 Ctrl+C 또는 GUI의 **종료**를 사용하고, 다시 실행하기 전에 핸들이 닫힐 때까지 1~2초 기다리세요.
3. **WinDivert 서비스를 중지하세요** (관리자 권한; LightSpeed가 자동으로 수행하는 것이 아니며 머신의 다른 모든 WinDivert 기반 앱에 영향을 줍니다. 실행 전에 다른 WinDivert 클라이언트가 없는지 확인하세요):
   ```powershell
   sc stop windivert
   ```
4. **완전 종료**는 최후의 수단이며, 외부 WinDivert 사용자를 배제할 수 없을 때만 사용합니다. Windows **다시 시작**은 이전 상태를 보유한 커널 세션을 재사용할 수 있으므로 완전한 **종료 → 전원 켜기**만이 머신 수준의 확실한 초기화입니다. 첫 번째 해결책이 아니며, 그런 이유 없이 LightSpeed가 재부팅을 권하지 않습니다.

WinDivert 2.2는 마지막 핸들을 닫은 뒤에도 드라이버를 로드된 상태로 유지합니다. WFP 공급자와 20개 하위 계층이 남아 있고(1 / 20), 서비스 중지 또는 재시작 전까지 `WinDivert64.sys`는 잠긴 상태로 남습니다. 이는 pin으로 바뀌지 않으며 Stop 시 드라이버가 언로드된다는 보장도 아닙니다.

> **팁:** v1.2.2 이전에는 별도의 버그(데이터 플레인 인증이 모든 패킷을 거부, issue #59)로 연결이 멈춰 사용자가 클라이언트를 반복적으로 강제 종료해야 했고, 이것이 대부분의 `FWP_E_IN_USE` 보고를 유발했습니다. 이 인증 버그는 v1.2.3에서 수정되었습니다.

---

## 윈도우 GUI 문제

이는 Windows의 `lightspeed-gui` 앱에 적용됩니다.

### Quit은 아무것도 하지 않고 좀비 프로세스를 남겼습니다.

v1.4.2 이전 버전에서는 트레이 메뉴에서 **종료**를 선택하면 프로세스가 백그라운드에서 실행되는 상태(좀비)로 남을 수 있으므로 창은 닫히지만 엔진은 계속 실행되고 나중에 실행하면 이상하게 작동합니다. v1.4.2에서는 이 문제가 수정되었습니다. 이제 Quit을 사용하면 프로세스가 완전히 종료됩니다.

이전 빌드를 사용 중이고 프로세스가 중단된 경우 수동으로 종료하세요.

```powershell
Get-Process lightspeed-gui -ErrorAction SilentlyContinue | Stop-Process
```

그런 다음 v1.4.2 이상으로 업그레이드하세요.

### 첫 번째 인스턴스에 초점을 맞추는 대신 두 번째 인스턴스가 열립니다.

v1.4.2 이전 버전에서는 GUI를 실행하면 두 번째 창과 두 번째 엔진이 두 번 쌓였습니다. 이제 GUI는 단일 인스턴스 가드를 보유합니다. 두 번째 실행에서는 간단한 "LightSpeed가 이미 실행 중입니다" 알림이 표시되고 다른 엔진을 시작하는 대신 종료됩니다. 두 번째 인스턴스를 강제로 실행하려면(진단을 위해) `--force`를 전달하거나 `LIGHTSPEED_GUI_FORCE=1`를 설정하세요.

### 릴레이가 발견되지 않았습니다.

GUI는 서명된 레지스트리를 통해 커뮤니티 릴레이를 검색합니다. 릴레이 목록이 비어 있는 경우:

1. 인터넷에 접속할 수 있는지, 방화벽이나 VPN이 레지스트리에 대한 아웃바운드 HTTPS를 차단하고 있지 않은지 확인하세요.
2. GUI 로그(아래 참조)에서 레지스트리 가져오기 또는 서명 확인 오류를 확인합니다.
3. CLI 빌드에서 `lightspeed-client --probe-proxies`를 실행하여 검색 및 프로브 보고서를 직접 확인합니다. CLI에서도 아무 것도 발견되지 않으면 문제는 GUI에 국한된 문제가 아니라 네트워크 측에 있는 것입니다.
4. 연결을 수정한 후 GUI를 다시 시작하십시오. 검색은 시작 시 실행됩니다.

### GUI 로그를 찾고 여는 방법

GUI는 추적 로그를 다음에 기록합니다.

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

해당 경로를 파일 탐색기 주소 표시줄에 붙여넣어 폴더를 연 다음 텍스트 편집기에서 `gui-trace.log`를 엽니다. 버그 보고서에 첨부하세요.

### 로그의 "Heartbeat 0 in"

`Heartbeat 0 in`와 같은 줄은 엔진이 현재 창에서 연결 유지 하트비트를 0번 보냈음을 의미합니다. 실제로는 클라이언트가 아직 작동 중인 제어 평면 연결을 설정하지 않았으므로 하트비트가 나가지 않은 경우에 표시됩니다. 일반적인 원인:

- 클라이언트가 아직 릴레이에 등록하지 않았습니다(상태 보기에서 등록 라인을 확인하십시오).
- 선택한 릴레이에 연결할 수 없습니다.
- 인터셉터가 시작되지 않았으므로 활성화된 세션이 없습니다.

등록이 성공하고 하트비트가 흐르기 시작하면 카운터가 올라갑니다. 릴레이가 정상으로 표시되는 동안 0으로 유지되면 `lightspeed-client --test-control`를 실행하여 제어 플레인에 연결할 수 있는지 여부를 격리합니다.

---

## 버그 신고 로그

자세한 진단을 캡처하려면 디버그 로깅을 실행하세요.

```bash
# CLI
RUST_LOG=debug lightspeed --start-interceptor --game rust --proxy YOUR_PROXY:4434 2>&1 | tee lightspeed.log

# Windows GUI
cd C:\path\to\lightspeed
lightspeed-gui.exe 2>&1 | tee lightspeed-log.txt
```

[GitHub 문제](https://github.com/ShibbityShwab/lightspeed/issues)에 로그 파일을 첨부하세요.

---

## 아직도 막혔나요?

- [FAQ](faq.md) - 자주 묻는 질문
- [GitHub 문제](https://github.com/ShibbityShwab/lightspeed/issues) - 기존 보고서 검색
- OS, 게임 및 로그 출력에 대한 새로운 문제를 엽니다.