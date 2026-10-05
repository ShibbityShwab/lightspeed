# LightSpeed 사용자 가이드

> LightSpeed로 핑을 줄이는 방법을 단계별로 설명합니다.

---

## LightSpeed는 어떻게 작동하나요

여러분의 ISP는 게임 트래픽을 속도가 아니라 비용에 맞춰 최적화된 경로로 보냅니다. LightSpeed는 게임의 UDP 패킷을 가로채 **릴레이**로 터널링합니다. 릴레이는 게임 서버 리전까지 고속 백본으로 연결된 데이터 센터의 가벼운 서버입니다. 기본값은 **커뮤니티 릴레이 네트워크**(후원사가 비용을 대는 8개의 릴레이, 서명된 레지스트리를 통해 자동으로 검색되며 별도 설정이 필요 없음)입니다. 직접 프록시를 자체 호스팅할 수도 있습니다. 그 경로가 ISP 기본 경로보다 빠르다면 핑이 내려갑니다.

```
Your PC ──→ ISP (slow path) ──→ Game Server        ❌ High ping
Your PC ──→ LightSpeed Proxy (fast backbone) ──→ Game Server   ✅ Low ping
```

---

## 사전 준비

- `lightspeed` CLI 도구 또는 `lightspeed-gui`(Windows). 프록시 설정은 필요하지 않습니다. 클라이언트가 커뮤니티 릴레이를 자동으로 찾아냅니다.
- 인터셉터 모드: root/관리자 권한
- 선택 사항: 자체 호스팅을 선호한다면 직접 만든 프록시 노드 ([프록시 배포](deploy-proxy.md) 참고)

---

## 어떤 앱이 필요한가요?

| 사용 중인 OS | 다운로드 | 이유 |
|-----------|----------|-----|
| **Windows** | `lightspeed-gui`(MSI 또는 ZIP) | GUI는 독립 실행 앱이며 클라이언트 엔진과 WinDivert 드라이버가 이미 들어 있습니다. CLI는 필요하지 **않습니다**. |
| **Linux** | `lightspeed-gui`(또는 `lightspeed-client`) | GUI는 Linux에서도 동작합니다(시스템 트레이는 스텁입니다). CLI는 고급 사용자용입니다. |
| **macOS** | `lightspeed-client` | 아직 테스트된 GUI가 없습니다. GUI는 macOS용으로 컴파일되지만 실제 하드웨어에서는 **테스트되지 않았습니다**. |
| **프록시 호스팅** | `lightspeed-proxy` | VPS에서 릴레이 노드를 운영할 때만 필요합니다. |

> **필요한 패키지는 항상 하나뿐입니다.** Windows 게이머라면 `lightspeed-gui`를 받고 나머지는 잊어도 됩니다. `lightspeed-client`는 Linux 고급 사용자와 macOS 게이머용이고, `lightspeed-proxy`는 자체 호스팅용입니다.

---

## 빠른 시작 (CLI, 모든 플랫폼)

### 1. 환경 확인

```bash
lightspeed --check
```

OS에 필요한 패킷 필터링 도구(Linux는 nftables/iptables, macOS는 pfctl, Windows는 WinDivert)가 있는지 확인합니다.

### 2. 릴레이 탐색

```bash
lightspeed --probe-proxies
```

검색된 각 릴레이까지의 지연 시간을 보여줍니다. 클라이언트는 첫 실행에서 가장 빠른 릴레이를 자동으로 고릅니다. 직접 지정하려면 내 위치가 아니라 **게임 서버**에 가장 가까운 릴레이를 고르세요.

### 3. 인터셉터 시작

```bash
# Linux/macOS (requires root)
sudo lightspeed --start-interceptor --game rust --proxy YOUR_PROXY_IP:4434

# Windows (requires Administrator)
lightspeed --start-interceptor --game rust --proxy YOUR_PROXY_IP:4434
```

### 4. 게임 실행

평소처럼 아무 서버에나 접속하세요. LightSpeed가 나가는 패킷에서 게임 서버를 자동으로 감지하고 몇 초 안에 터널링을 시작합니다.

### 5. 모니터링

CLI는 실시간 통계를 표시합니다.
```
⚡ OPTIMIZING - 123.45.67.89:28015
Packets Sent: 142 | Packets Returned: 139 | Packets Delivered: 139
```

---

## 빠른 시작 (GUI, Windows)

### 1. 다운로드

[Releases](https://github.com/ShibbityShwab/lightspeed/releases)에서 최신 릴리스를 받으세요. 모든 파일을 압축 해제하고 `WinDivert64.sys`와 `WinDivert.dll`을 `lightspeed-gui.exe` 옆에 두세요.

### 2. 관리자 권한으로 실행

`lightspeed-gui.exe`를 마우스 오른쪽 버튼으로 클릭 → **관리자 권한으로 실행**. 인터셉터는 커널 수준 접근이 필요합니다(VPN 소프트웨어와 같습니다).

### 3. 릴레이와 게임 선택

GUI가 커뮤니티 릴레이를 찾아내고 첫 실행에서 가장 빠른 것을 자동으로 고릅니다. 드롭다운에서 릴레이를 직접 바꾼 뒤 게임을 고르세요.

### 4. **⚡ OPTIMIZE MY ROUTE** 클릭

상태가 "🎯 Finding your game server…"로 바뀝니다.

### 5. 게임 실행

아무 서버에나 접속하세요. LightSpeed가 몇 초 안에 자동으로 감지합니다.

---

### macOS GUI (테스트되지 않음)

GUI는 macOS용으로 컴파일되지만 실제 하드웨어에서는 **테스트되지 않았습니다**. 릴리스는
`.app`/`.dmg` 없이 `tar.xz`만 제공합니다(cargo-dist 0.32에는 `.app`/`.dmg` 지원이 없습니다).
제대로 된 번들을 만들려면 Mac에서 다음을 실행하세요.

```bash
cargo build --release -p lightspeed-gui
./tools/package-macos.sh 1.6.5
```

그러면 `LightSpeed.app`과 `LightSpeed-1.6.5.dmg`가 만들어집니다. 앱은 임시 서명되어
있으므로 첫 실행 시 마우스 오른쪽 버튼 → 열기(또는
`xattr -dr com.apple.quarantine LightSpeed.app`)가 필요합니다.

---

## 알맞은 릴레이 고르기

| 내 위치 | 게임 서버 위치 | 최적 릴레이 리전 |
|-----------|---------------|-------------------|
| 호주 | 미국 서부 | 미국 서부(로스앤젤레스) |
| 유럽 | 미국 동부 | 미국 동부(뉴저지) |
| 동남아시아 | 싱가포르 | 싱가포르 |
| 남아시아 | 인도 | 뭄바이 |
| 동아시아 | 일본 | 도쿄 |
| 남미 | 미국 동부 | 미국 동부(뉴저지) |
| 어디든 | 같은 리전 | 게임 서버에 가장 가까운 곳 |

> **경험 법칙:** 내 위치가 아니라 **게임 서버**에 가장 가까운 릴레이를 고르세요. 트래픽은 PC → 릴레이 → 게임 서버 순서로 흐르므로, 릴레이에서 게임 서버까지의 구간이 가장 중요합니다.

---

## 순방향 오류 정정(FEC)

FEC는 재전송 없이 손실된 패킷을 복구하기 위해 대역폭을 약 25% 더 씁니다.

**켜야 할 때:**
- 패킷 손실이 있을 때(순간 끊김, 캐릭터 되돌아감)
- 간헐적 간섭이 있는 Wi-Fi를 쓸 때

**꺼야 할 때:**
- 연결이 이미 포화 상태일 때
- 종량제/용량 제한 연결일 때
- 패킷 손실이 0.1% 미만일 때(이득 없음)

```bash
# CLI: enable FEC with default block size (K=4)
lightspeed --start-interceptor --game cs2 --proxy YOUR_PROXY:4434 --fec

# Custom block size (K=8 → 12.5% overhead)
lightspeed --start-interceptor --game cs2 --proxy YOUR_PROXY:4434 --fec --fec-k 8
```

---

## 고급: 수동 서버 모드

자동 감지가 되지 않을 때(사용자 지정 포트, 특이한 게임) 씁니다.

```bash
# Redirect mode: game connects to localhost, LightSpeed forwards to real server
lightspeed --game rust --game-server 123.45.67.89:28015 --proxy YOUR_PROXY:4434
```

그런 다음 게임이 `127.0.0.1:<port>`로 접속하도록 설정하세요. 이 포트는 LightSpeed가 출력하는 로컬 포트입니다.

---

## 세션 중 서버 전환

LightSpeed는 한 서버에서 접속이 끊기고 다른 서버로 접속할 때 이를 자동으로 감지합니다. 상태에 잠깐 "🎯 Finding your game server…"가 표시되다가 새 목적지에 맞춰집니다. 직접 할 일은 없습니다.

---

## 시스템 트레이 (Windows GUI)

- **×**를 누르면 트레이로 최소화됩니다(종료되지 않음)
- 번개 아이콘을 두 번 클릭하면 복원됩니다
- 마우스 오른쪽 버튼으로 연결/해제/종료를 빠르게 실행할 수 있습니다

---

## 함께 보기

- [CLI 레퍼런스](CLI-REFERENCE.md) - 모든 플래그 설명
- [FAQ](faq.md) - 자주 묻는 질문
- [문제 해결](troubleshooting.md) - 문제 고치기
- [프록시 배포](deploy-proxy.md) - 직접 프록시 운영하기
- [지원 게임](supported-games.md) - 게임 호환성
