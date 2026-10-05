# Windows에 LightSpeed 설치하기

Windows는 GUI 우선 플랫폼입니다. `lightspeed-gui` 패키지는 클라이언트 엔진과 WinDivert 드라이버를 이미 담고 있는 독립 실행 앱이므로 별도로 클라이언트를 받을 필요가 없습니다.

---

## 무엇을 받아야 하나요

[Releases 페이지](https://github.com/ShibbityShwab/lightspeed/releases/latest)에서 최신 릴리스를 받으세요. Windows x86_64(현재의 모든 Intel 및 AMD PC) 환경이라면 둘 중 하나를 고르세요.

| 파일 | 언제 쓰나요 |
|------|-------------|
| `lightspeed-gui-...-windows-msvc.msi` | 권장합니다. Program Files에 설치하고 시작 메뉴 바로 가기를 추가하며 제거 프로그램을 등록합니다. |
| `lightspeed-gui-...-windows-msvc.zip` | 포터블. 아무 곳에나 압축을 풀고 `lightspeed-gui.exe`를 실행하세요. |

둘 다 같은 GUI와 `WinDivert.dll`, `WinDivert64.sys`를 담고 있습니다.

> **Windows on ARM64는 아직 공식 배포 대상이 아닙니다.** 릴리스 빌드는 `x86_64-pc-windows-msvc`를 대상으로 합니다. ARM64 기기에서는 x86_64 빌드가 Windows 에뮬레이션 계층에서 실행되지만, 실제 ARM64 하드웨어에서는 테스트되지 않았습니다.

### Windows CLI (지원하지 않음)

Windows 명령줄 빌드도 zip(`lightspeed-client-...-windows-msvc.zip`)으로 배포됩니다. 스크립트와 헤드리스 용도로 제공되지만 **지원하지 않습니다**. Windows에서는 GUI를 권장합니다. 그래도 쓴다면 관리자 권한 터미널에서 실행하세요(아래 참고).

---

## 설치 (MSI)

1. `.msi`를 다운로드합니다.
2. 두 번 클릭하고 마법사를 따라가세요. Windows SmartScreen이 알 수 없는 게시자라고 경고할 수 있습니다. 릴리스는 Sigstore로 증명(attestation)되어 있으니 원한다면 증명을 검증할 수 있습니다.
3. 시작 메뉴에서 **LightSpeed**를 실행하세요.

## 설치 (포터블 zip)

1. `.zip`을 다운로드합니다.
2. 마우스 오른쪽 버튼으로 클릭하고 **모두 압축 풀기**를 골라 쓰기 권한이 있는 폴더(예: `C:\LightSpeed`)에 푸세요. zip 안에서 바로 실행하지 마세요.
3. `lightspeed-gui.exe`를 실행하세요.

---

## 관리자 권한

LightSpeed는 WinDivert 드라이버로 게임 UDP 트래픽을 가로챕니다. WinDivert는 **관리자** 권한이 필요합니다.

- GUI는 인터셉터를 시작해야 할 때 권한 상승을 요청합니다. UAC 프롬프트를 수락하세요.
- CLI 빌드를 쓴다면 **관리자** 터미널에서 실행하세요(Windows Terminal이나 PowerShell을 마우스 오른쪽 버튼으로 클릭한 뒤 **관리자 권한으로 실행**).
- 권한 상승 없이는 인터셉터가 붙을 수 없고 상태 화면에 인터셉터 오류가 표시됩니다.

---

## 첫 실행

1. GUI를 실행하고 UAC 프롬프트를 수락하세요.
2. GUI가 서명된 레지스트리를 통해 커뮤니티 릴레이를 자동으로 찾아냅니다. 프록시 주소는 필요하지 않습니다.
3. 게임 목록에서 게임을 고르세요.
4. 인터셉터를 시작한 뒤 게임을 실행하고 서버에 접속하세요.

GUI는 릴레이 상태, 등록 상태, 패킷 카운터를 보여주므로 트래픽이 흐르는 것을 확인할 수 있습니다.

---

## 제대로 동작하는지 확인

**릴레이 검색과 등록을 확인하세요.** GUI 상태 화면을 여세요. 커뮤니티 릴레이(로스앤젤레스, 뉴저지, 싱가포르, 프랑크푸르트, 도쿄, 뭄바이, 마드리드, 시드니)가 정상 상태로 나열되고, QUIC/auth 핸드셰이크가 성공했다는 등록 줄이 보여야 합니다. CLI 빌드를 쓴다면 다음을 실행하세요.

```powershell
lightspeed-client.exe --probe-proxies
```

검색/탐색을 한 번 수행하고 발견된 각 릴레이와 지연 시간을 나열한 보고서를 출력합니다. 커뮤니티 릴레이 8개가 모두 보여야 합니다.

**컨트롤 플레인 등록을 확인하세요.** CLI 빌드에서는 다음을 실행하세요.

```powershell
lightspeed-client.exe --test-control
```

QUIC 컨트롤 플레인에 접속하고 세션을 등록하고 핑을 보낸 뒤 연결을 끊으면서 각 단계의 결과를 출력합니다. 등록에 성공하면 컨트롤 플레인에 도달할 수 있고 인증이 동작한다는 뜻입니다.

**패킷 흐름을 확인하세요.** GUI에서 게임이 서버에 접속하면 패킷 카운터가 올라가야 합니다. "Packets Sent"가 0에 머물면 인터셉터가 아직 게임 트래픽을 보지 못한 것입니다. [문제 해결](troubleshooting.md)을 참고하세요.

---

## 로그

GUI는 추적 로그를 다음 위치에 기록합니다.

```
%LOCALAPPDATA%\Lightspeed\gui-trace.log
```

파일 탐색기 주소 표시줄에 이 경로를 붙여 넣으면 열립니다. 문제가 생기면 버그 보고에 첨부하세요. 로그 줄의 의미는 [문제 해결](troubleshooting.md)을 참고하세요.

---

## 제거

- **MSI:** 설정 → 앱 → 설치된 앱 → LightSpeed → 제거.
- **Zip:** 압축을 푼 폴더를 삭제하세요. `%LOCALAPPDATA%\Lightspeed\` 아래의 로그 파일은 남습니다. 깔끔하게 지우고 싶다면 직접 삭제하세요.

---

## 다음 단계

- [지원 게임](supported-games.md)
- [문제 해결](troubleshooting.md)
- [CLI 레퍼런스](CLI-REFERENCE.md)
