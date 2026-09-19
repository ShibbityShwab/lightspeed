#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — In-place handoff end-to-end test (Linux, root only)
#
# Proves a running relay can execve a new binary in place (same PID)
# while an existing client session and its upstream flow survive:
#   * a real data packet round-trips through the relay before the handoff
#   * SIGUSR2 triggers the validated in-place exec (1.4.4 -> 1.4.5)
#   * the PID is unchanged and /health reports the new version
#   * the SAME client socket round-trips again after the handoff
#   * the game server sees the SAME outbound source port (fd preserved)
#
# Requires: Linux, root (the proxy's request/manifest paths are root-owned
# under /run/lightspeed), and a second-versioned proxy binary.
#
# Usage:
#   sudo LIGHTSPEED_HANDOFF_E2E_NEW_BIN=/path/to/1.4.5/lightspeed-proxy \
#        bash infra/scripts/test_handoff_e2e.sh
#
# Env:
#   LIGHTSPEED_HANDOFF_E2E_NEW_BIN  Prebuilt newer-version proxy (skips a build)
#   LIGHTSPEED_HANDOFF_E2E_REPO     Repo root (default: script's ../..)
# ──────────────────────────────────────────────────────────────
set -euo pipefail

[ "$(uname -s)" = "Linux" ] || { echo "Linux only"; exit 0; }
[ "$(id -u)" -eq 0 ] || { echo "must run as root (use sudo)"; exit 1; }

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="${LIGHTSPEED_HANDOFF_E2E_REPO:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
OLD_BIN="$REPO/target/debug/lightspeed-proxy"
[ -x "$OLD_BIN" ] || { echo "build the proxy first: cargo build -p lightspeed-proxy"; exit 1; }

TMP="$(mktemp -d /tmp/ls-handoff-e2e.XXXXXX)"
DPORT=15434; CPORT=15433; HPORT=18084
RUN_DIR="/run/lightspeed"
RUN_DIR_EXISTED=0
if [ -e "$RUN_DIR" ]; then RUN_DIR_EXISTED=1; fi
cleanup() {
    for p in "$HPORT" "$DPORT" "$CPORT"; do fuser -k -n udp "$p" 2>/dev/null || true; done
    # Remove only the files this test writes; never blow away a pre-existing
    # /run/lightspeed that other state may live in.
    rm -f "$RUN_DIR/handoff-request.json" "$RUN_DIR/handoff.json" "$RUN_DIR/handoff-result.json"
    if [ "$RUN_DIR_EXISTED" -eq 0 ]; then
        rmdir "$RUN_DIR" 2>/dev/null || true
    fi
    rm -rf "$TMP"
}
trap cleanup EXIT

# 1. New (newer-version) binary: use the provided one, else build a +1 copy.
NEW_BIN="${LIGHTSPEED_HANDOFF_E2E_NEW_BIN:-}"
if [ -z "$NEW_BIN" ]; then
    echo "building a newer-version proxy in a temp copy..."
    NEW_SRC="$TMP/src"; rsync -a --exclude target --exclude .git "$REPO/" "$NEW_SRC/"
    cur="$(grep -m1 '^version' "$NEW_SRC/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')"
    nxt="$(echo "$cur" | awk -F. '{printf "%s.%s.%d",$1,$2,$3+1}')"
    sed -i "s/^version = \"$cur\"/version = \"$nxt\"/" "$NEW_SRC/Cargo.toml"
    ( cd "$NEW_SRC" && CARGO_TARGET_DIR="$TMP/target" cargo build -q -p lightspeed-proxy )
    NEW_BIN="$TMP/target/debug/lightspeed-proxy"
fi
[ -x "$NEW_BIN" ] || { echo "new binary not executable: $NEW_BIN"; exit 1; }

mkdir -p "$TMP/releases/1.4.4" "$TMP/releases/1.4.5" "$TMP/tls"
install -m 0755 "$OLD_BIN" "$TMP/releases/1.4.4/lightspeed-proxy"
install -m 0755 "$NEW_BIN" "$TMP/releases/1.4.5/lightspeed-proxy"
cat > "$TMP/proxy.toml" <<EOF
[server]
node_id = "handoff-e2e"
region = "test"
max_clients = 10
[network]
data_port = $DPORT
control_port = $CPORT
health_port = $HPORT
[security]
require_auth = false
EOF

echo "running the handoff E2E..."
TMP="$TMP" DPORT="$DPORT" CPORT="$CPORT" HPORT="$HPORT" python3 - <<'PY'
import hashlib, json, os, signal, socket, struct, subprocess, threading, time, urllib.request, sys
tmp=os.environ["TMP"]; dport=int(os.environ["DPORT"]); cport=int(os.environ["CPORT"]); hport=int(os.environ["HPORT"])
OLD=f"{tmp}/releases/1.4.4/lightspeed-proxy"; NEW=f"{tmp}/releases/1.4.5/lightspeed-proxy"
HEALTH=f"http://127.0.0.1:{hport}/health"; DATA=("127.0.0.1",dport); GAME=55557
def sha(p):
    h=hashlib.sha256()
    with open(p,'rb') as f:
        for c in iter(lambda:f.read(1<<20),b''): h.update(c)
    return h.hexdigest()
seen=[]
gs=socket.socket(socket.AF_INET,socket.SOCK_DGRAM); gs.bind(("127.0.0.1",GAME))
def echo():
    while True:
        try: data,addr=gs.recvfrom(2048)
        except OSError: return
        seen.append((data,addr[1])); gs.sendto(data,addr)
threading.Thread(target=echo,daemon=True).start()
client=socket.socket(socket.AF_INET,socket.SOCK_DGRAM); client.bind(("127.0.0.1",40001)); client.settimeout(3)
def pkt(payload,seq):
    b=bytearray(24); b[0]=0x30  # PROTOCOL_VERSION 3, flags 0
    struct.pack_into(">H",b,5,seq); struct.pack_into(">I",b,7,int(time.time()*1e6)&0xffffffff)
    b[11:15]=socket.inet_aton("127.0.0.1"); b[15:19]=socket.inet_aton("127.0.0.1")
    struct.pack_into(">H",b,19,40001); struct.pack_into(">H",b,21,GAME)
    return bytes(b)+payload
os.makedirs("/run/lightspeed",exist_ok=True)
req={"schema_version":1,"handoff_id":"e2e","version":"1.4.5","binary_path":NEW,"sha256":sha(NEW),"requested_at_unix_ms":int(time.time()*1000)}
with open("/run/lightspeed/handoff-request.json","w") as f: json.dump(req,f)
os.chmod("/run/lightspeed/handoff-request.json",0o600)
env=dict(os.environ); env["LIGHTSPEED_RELEASES_DIR"]=f"{tmp}/releases"; env["LIGHTSPEED_TLS_DIR"]=f"{tmp}/tls"; env.pop("LIGHTSPEED_HANDOFF_MANIFEST",None)
log=open(f"{tmp}/proxy.log","w")
proc=subprocess.Popen([OLD,"--config",f"{tmp}/proxy.toml","--data-bind",f"127.0.0.1:{dport}","--control-bind",f"127.0.0.1:{cport}","--health-bind",f"127.0.0.1:{hport}","--dev"],env=env,stdout=log,stderr=subprocess.STDOUT)
def health():
    with urllib.request.urlopen(HEALTH,timeout=1) as r: return json.load(r)
h=None
for _ in range(60):
    try: h=health(); break
    except Exception: time.sleep(0.25)
if not h: print("FAIL: no health"); proc.kill(); sys.exit(1)
client.sendto(pkt(b"before",1),DATA); r1,_=client.recvfrom(2048)
if r1[24:]!=b"before": print("FAIL: pre-handoff roundtrip"); proc.kill(); sys.exit(1)
print(f"  pre-handoff roundtrip ok (game src port {seen[-1][1]})")
pid=proc.pid; os.kill(pid,signal.SIGUSR2)
after=None
for _ in range(80):
    try:
        x=health()
        if x.get("version")=="1.4.5": after=x; break
    except Exception: pass
    time.sleep(0.25)
if not after: print("FAIL: version did not switch to 1.4.5"); print(open(f"{tmp}/proxy.log").read()[-2000:]); proc.kill(); sys.exit(1)
last=(after.get("handoff") or {}).get("last") or {}
print(f"  after exec: pid={proc.pid} version={after.get('version')} result={last.get('result')} sessions={last.get('sessions_transferred')} handoff_id={last.get('handoff_id')}")
client.sendto(pkt(b"after",2),DATA); r2,_=client.recvfrom(2048)
if r2[24:]!=b"after": print("FAIL: post-handoff roundtrip"); proc.kill(); sys.exit(1)
same_pid=proc.pid==pid; same_port=seen[0][1]==seen[-1][1]; same_id=last.get("handoff_id")=="e2e"
if not same_id: print(f"FAIL: handoff_id not propagated (got {last.get('handoff_id')!r}, want 'e2e')")
ok = same_pid and same_port and same_id and last.get("result")=="ok" and int(last.get("sessions_transferred") or 0)>=1
print(f"  post-handoff roundtrip ok; outbound src port {seen[0][1]} -> {seen[-1][1]}")
proc.terminate()
try: proc.wait(timeout=5)
except Exception: proc.kill()
print("HANDOFF_E2E_PASS" if ok else "HANDOFF_E2E_FAIL")
sys.exit(0 if ok else 1)
PY
