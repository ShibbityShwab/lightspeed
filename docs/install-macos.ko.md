# macOS에 LightSpeed 설치하기

> [!WARNING]
> 기계 번역이며 원어민의 검수를 받지 않았습니다. [영어 원문](install-macos.md)이 기준입니다.

macOS는 CLI 우선 플랫폼입니다. GUI는 macOS용으로 컴파일되지만 실제 하드웨어에서는 테스트되지 않았으므로, 명령줄 클라이언트(`lightspeed-client`)가 지원 대상 경로입니다.

---

## 무엇을 받아야 하나요

[Releases 페이지](https://github.com/ShibbityShwab/lightspeed/releases/latest)에서 최신 릴리스를 받으세요. Mac의 CPU에 맞는 압축 파일을 고르세요.

| 사용 중인 Mac | 타깃 | 파일 |
|----------|--------|------|
| Apple Silicon(M1 이후) | `aarch64-apple-darwin` | `lightspeed-client-...-aarch64-apple-darwin.tar.xz` |
| Intel | `x86_64-apple-darwin` | `lightspeed-client-...-x86_64-apple-darwin.tar.xz` |

어떤 것인지 모르겠다면 다음을 실행하세요.

```bash
uname -m
```

`arm64`는 Apple Silicon, `x86_64`는 Intel입니다.

---

## 설치

셸 설치 프로그램이 가장 쉬운 방법입니다. 아키텍처를 감지하고 클라이언트를 설치합니다.

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ShibbityShwab/lightspeed/releases/latest/download/lightspeed-client-installer.sh | sh
```

압축 파일에서 직접 설치해도 됩니다.

```bash
# Replace the filename with the one you downloaded.
tar -xf lightspeed-client-...-aarch64-apple-darwin.tar.xz
sudo mv lightspeed-client /usr/local/bin/
```

바이너리가 실행되는지 확인하세요.

```bash
lightspeed-client --version
```

---

## root 권한

macOS 인터셉터는 `pfctl`(내장 패킷 필터)로 게임 UDP 트래픽을 리디렉션합니다. `pfctl`은 root가 필요하므로 인터셉터를 시작할 때는 `sudo`로 클라이언트를 실행하세요.

```bash
sudo lightspeed-client --start-interceptor --game rust
```

검색, 탐색, 진단(`--probe-proxies`, `--test-control`, `--check`)에는 root가 필요하지 않습니다.

---

## 첫 실행

클라이언트가 서명된 레지스트리를 통해 커뮤니티 릴레이를 자동으로 찾아냅니다. 프록시 주소는 필요하지 않습니다.

```bash
# Probe the community relays and print a report
lightspeed-client --probe-proxies

# Start the interceptor for your game
sudo lightspeed-client --start-interceptor --game rust
```

---

## 제대로 동작하는지 확인

**릴레이 검색을 확인하세요.** 다음을 실행하세요.

```bash
lightspeed-client --probe-proxies
```

검색/탐색을 한 번 수행하고 발견된 각 릴레이와 지연 시간을 나열한 보고서를 출력합니다. 커뮤니티 릴레이 8개(로스앤젤레스, 뉴저지, 싱가포르, 프랑크푸르트, 도쿄, 뭄바이, 마드리드, 시드니)가 모두 보여야 합니다.

**컨트롤 플레인 등록을 확인하세요.** 다음을 실행하세요.

```bash
lightspeed-client --test-control
```

QUIC 컨트롤 플레인에 접속하고 세션을 등록하고 핑을 보낸 뒤 연결을 끊으면서 각 단계의 결과를 출력합니다. 등록에 성공하면 컨트롤 플레인에 도달할 수 있고 인증이 동작한다는 뜻입니다.

**환경을 확인하세요.** 다음을 실행하세요.

```bash
lightspeed-client --check
```

인터셉터 사용 가능 여부, root 상태, 게임 감지, 프록시 연결 가능성을 보고합니다.

**패킷 흐름을 확인하세요.** 인터셉터가 실행 중이고 게임이 서버에 접속하면 클라이언트의 패킷 카운터가 올라가야 합니다. "Packets Sent"가 0에 머물면 인터셉터가 아직 게임 트래픽을 보지 못한 것입니다. [문제 해결](troubleshooting.md)을 참고하세요.

---

## Gatekeeper 관련 참고

macOS가 "cannot be opened because the developer cannot be verified"라며 바이너리를 막으면 격리 속성을 지우세요.

```bash
xattr -d com.apple.quarantine /usr/local/bin/lightspeed-client
```

공식 Releases 페이지에서 바이너리를 받은 경우에만 이렇게 하세요.

---

## 다음 단계

- [지원 게임](supported-games.md)
- [문제 해결](troubleshooting.md)
- [CLI 레퍼런스](CLI-REFERENCE.md)
