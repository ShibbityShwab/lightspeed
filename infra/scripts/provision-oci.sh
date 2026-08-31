#!/bin/bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Provision OCI Always-Free Relay Nodes
#
# One-click provisioning of LightSpeed proxy relays on Oracle
# Cloud Infrastructure (OCI) Always Free tier. This is the
# recommended $0 relay host — see infra/README.md "Choosing a
# Region" for why (10 TB/mo egress, home-region-only, 2 OCPU ARM
# + 2 x86 micro per tenancy).
#
# Usage:
#   ./provision-oci.sh [--list-regions] [region...]
#
# Examples:
#   ./provision-oci.sh us-east-1 eu-frankfurt-1 ap-tokyo-1
#   ./provision-oci.sh --list-regions
#
# Prerequisites:
#   - oci-cli installed and configured (oci setup config)
#   - jq
#   - An SSH public key at ~/.ssh/id_ed25519.pub (or set OCI_SSH_KEY)
#   - A compartment (defaults to the tenancy root)
#
# NOTE: reference implementation. Always-Free capacity and oci-cli
# output shapes vary, so smoke-test against a real account before
# relying on it. Network resources (VCN/subnet/IGW/security list)
# are created idempotently per region.
# ──────────────────────────────────────────────────────────────
set -euo pipefail

OCI_SSH_KEY="${OCI_SSH_KEY:-$HOME/.ssh/id_ed25519.pub}"
COMPARTMENT="${OCI_COMPARTMENT_OCID:-}"
ARM_OCPUS=1
ARM_MEM_GB=6

GREEN='\033[0;32m'; RED='\033[0;31m'; CYAN='\033[0;36m'; NC='\033[0m'

# oci-cli wrapper: pins the region for the current loop iteration.
oc() { command oci --region "$REGION" "$@"; }

# Game-server region shorthand → OCI home region.
declare -A REGION_MAP=(
  ["us-east-1"]="us-ashburn-1"   ["us-east"]="us-ashburn-1"   ["ewr"]="us-ashburn-1"
  ["us-west-1"]="us-phoenix-1"   ["us-west"]="us-phoenix-1"
  ["eu-central-1"]="eu-frankfurt-1" ["eu-central"]="eu-frankfurt-1" ["fra"]="eu-frankfurt-1"
  ["eu-west-1"]="eu-amsterdam-1" ["eu-west"]="eu-amsterdam-1" ["ams"]="eu-amsterdam-1"
  ["ap-northeast-1"]="ap-tokyo-1" ["ap-northeast"]="ap-tokyo-1" ["nrt"]="ap-tokyo-1"
  ["ap-northeast-2"]="ap-seoul-1" ["ap-seoul"]="ap-seoul-1"
  ["ap-southeast-1"]="ap-singapore-1" ["ap-southeast"]="ap-singapore-1" ["sgp"]="ap-singapore-1"
  ["sa-east-1"]="sa-saopaulo-1"  ["sa-east"]="sa-saopaulo-1"
)

list_regions() {
  echo "OCI Always-Free regions (game-region → OCI region):"
  echo "  us-east-1 / ewr      → us-ashburn-1   (US-East)"
  echo "  us-west-1            → us-phoenix-1   (US-West)"
  echo "  eu-central-1 / fra   → eu-frankfurt-1 (EU-Central)"
  echo "  eu-west-1 / ams      → eu-amsterdam-1 (EU-West)"
  echo "  ap-northeast-1 / nrt → ap-tokyo-1     (Japan)"
  echo "  ap-northeast-2       → ap-seoul-1     (Korea)"
  echo "  ap-southeast-1 / sgp → ap-singapore-1 (SEA)"
  echo "  sa-east-1            → sa-saopaulo-1  (South America)"
  echo ""
  echo "Remember: Always-Free compute must be in your HOME region."
  echo "One tenancy = one home region. Covering N regions needs N accounts."
}

tenancy_ocid() {
  command oci --region "$REGION" iam region-subscription list \
    --query 'data[0]."tenancy-id"' --raw-output 2>/dev/null
}

# ── Cloud-init startup script (quoted heredoc; __NODE_ID__/__REGION__ templated) ──
startup_script() {
  cat << 'CLOUDINIT'
#!/bin/bash
set -euo pipefail
for i in $(seq 1 30); do curl -sf -o /dev/null https://github.com && break; sleep 5; done
apt-get update -qq && apt-get install -y -qq curl ufw >/dev/null

case "$(uname -m)" in
  aarch64) TARGET="aarch64-unknown-linux-gnu" ;;
  x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
  *) echo "unsupported arch"; exit 1 ;;
esac
FILE="lightspeed-proxy-${TARGET}.tar.xz"
URL="https://github.com/ShibbityShwab/lightspeed/releases/latest/download/$FILE"
cd /tmp
curl -sL "$URL" -o "$FILE"
curl -sL "$URL.sha256" -o "$FILE.sha256"
sha256sum -c "$FILE.sha256" || { echo "proxy checksum verification failed"; exit 1; }
rm -rf /tmp/proxy-extract && mkdir -p /tmp/proxy-extract
tar -xJf "$FILE" -C /tmp/proxy-extract
BIN=$(find /tmp/proxy-extract -type f -name lightspeed-proxy | head -1)
install -m755 "$BIN" /usr/local/bin/lightspeed-proxy

mkdir -p /etc/lightspeed
cat > /etc/lightspeed/proxy.toml << EOF
[server]
node_id     = "__NODE_ID__"
region      = "__REGION__"
max_clients = 100

[security]
require_auth                = true
max_amplification_ratio     = 2.0
max_destinations_per_window = 10
ban_duration_secs           = 3600

[rate_limit]
max_pps_per_client = 1000
max_bps_per_client = 1000000

[metrics]
enabled       = true
interval_secs = 10
EOF

cat > /etc/systemd/system/lightspeed-proxy.service << 'UNIT'
[Unit]
Description=LightSpeed Proxy
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
Restart=always
RestartSec=5
ExecStart=/usr/local/bin/lightspeed-proxy --config /etc/lightspeed/proxy.toml --data-bind 0.0.0.0:4434 --control-bind 0.0.0.0:4433 --health-bind 0.0.0.0:8080
DynamicUser=true
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadOnlyPaths=/etc/lightspeed
PrivateTmp=true
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
UNIT

# Node identity (Ed25519) for future community-registry registration.
if [ ! -f /etc/lightspeed/node.key ]; then
  ssh-keygen -t ed25519 -N "" -f /etc/lightspeed/node.key -C "lightspeed-node" >/dev/null
fi
echo "NODE_PUBKEY=$(cut -d' ' -f2 /etc/lightspeed/node.key.pub)" > /etc/lightspeed/identity

systemctl daemon-reload
systemctl enable --now lightspeed-proxy

ufw allow 22/tcp >/dev/null 2>&1 || true
ufw allow 8080/tcp >/dev/null 2>&1 || true
ufw allow 4434/udp >/dev/null 2>&1 || true
ufw allow 4433/udp >/dev/null 2>&1 || true
ufw --force enable >/dev/null 2>&1 || true
CLOUDINIT
}

# ── Network helpers (idempotent) ─────────────────────────────
ensure_network() {
  local vcn subnet igw rt sl
  vcn=$(oc network vcn list -c "$COMPARTMENT" --query "data[?\"display-name\"=='lightspeed-vcn'].id | [0]" --raw-output 2>/dev/null)
  if [ -z "$vcn" ] || [ "$vcn" = "None" ] || [ "$vcn" = "null" ]; then
    echo "    creating VCN..."
    vcn=$(oc network vcn create -c "$COMPARTMENT" --cidr-block 10.0.0.0/16 --display-name lightspeed-vcn --query 'data.id' --raw-output)
  fi

  subnet=$(oc network subnet list -c "$COMPARTMENT" --vcn-id "$vcn" --query "data[?\"display-name\"=='lightspeed-subnet'].id | [0]" --raw-output 2>/dev/null)
  if [ -z "$subnet" ] || [ "$subnet" = "None" ] || [ "$subnet" = "null" ]; then
    echo "    creating subnet..."
    subnet=$(oc network subnet create -c "$COMPARTMENT" --vcn-id "$vcn" --cidr-block 10.0.0.0/24 --display-name lightspeed-subnet --query 'data.id' --raw-output)
  fi

  igw=$(oc network internet-gateway list -c "$COMPARTMENT" --vcn-id "$vcn" --query "data[?\"display-name\"=='lightspeed-igw'].id | [0]" --raw-output 2>/dev/null)
  if [ -z "$igw" ] || [ "$igw" = "None" ] || [ "$igw" = "null" ]; then
    echo "    creating internet gateway..."
    igw=$(oc network internet-gateway create -c "$COMPARTMENT" --vcn-id "$vcn" --is-enabled true --display-name lightspeed-igw --query 'data.id' --raw-output)
  fi

  rt=$(oc network route-table list -c "$COMPARTMENT" --vcn-id "$vcn" --query 'data[0].id' --raw-output 2>/dev/null)
  oc network route-table update --rt-id "$rt" \
    --route-rules "[{\"destination\":\"0.0.0.0/0\",\"networkEntityId\":\"$igw\"}]" >/dev/null 2>&1 || true

  sl=$(oc network security-list list -c "$COMPARTMENT" --vcn-id "$vcn" --query 'data[0].id' --raw-output 2>/dev/null)
  oc network security-list update --security-list-id "$sl" \
    --ingress-security-rules '[
      {"source":"0.0.0.0/0","protocol":"6","tcpOptions":{"destinationPortRange":{"min":22,"max":22}}},
      {"source":"0.0.0.0/0","protocol":"6","tcpOptions":{"destinationPortRange":{"min":8080,"max":8080}}},
      {"source":"0.0.0.0/0","protocol":"17","udpOptions":{"destinationPortRange":{"min":4433,"max":4434}}}
    ]' >/dev/null 2>&1 || true

  echo "$subnet"
}

lookup_image() {
  local shape="$1"
  oc compute image list -c "$COMPARTMENT" \
    --operating-system "Canonical Ubuntu" --operating-system-version "24.04" \
    --shape "$shape" --sort-by TIMECREATED \
    --query 'data[0].id' --raw-output 2>/dev/null
}

launch() {
  local name region subnet ad image shape instance
  name="$1"; region="$2"; subnet="$3"
  ad=$(oc iam availability-domain list -c "$COMPARTMENT" --query 'data[0].name' --raw-output)

  for shape in "VM.Standard.A1.Flex" "VM.Standard.E2.1.Micro"; do
    image=$(lookup_image "$shape")
    [ -n "$image" ] && [ "$image" != "None" ] && [ "$image" != "null" ] || continue
    shape_config=()
    [ "$shape" = "VM.Standard.A1.Flex" ] && shape_config=(--shape-config "{\"ocpus\":$ARM_OCPUS,\"memoryInGBs\":$ARM_MEM_GB}")

    echo "    launching $shape ..."
    if instance=$(oc compute instance launch \
        --availability-domain "$ad" -c "$COMPARTMENT" \
        --shape "$shape" "${shape_config[@]}" \
        --subnet-id "$subnet" --image-id "$image" \
        --assign-public-ip true \
        --ssh-authorized-keys-file "$OCI_SSH_KEY" \
        --metadata "{\"user_data\":\"$(startup_script | sed "s/__NODE_ID__/$name/; s/__REGION__/$region/" | base64 | tr -d '\n')\"}" \
        --display-name "$name" --wait-for-state RUNNING \
        --query 'data.id' --raw-output 2>/dev/null); then
      echo "$instance"
      return 0
    fi
    echo "    $shape failed (out of capacity?); trying next shape..."
  done
  return 1
}

# ── Main ─────────────────────────────────────────────────────
if [ $# -eq 0 ] || [ "$1" = "--list-regions" ]; then
  list_regions
  [ "$1" = "--list-regions" ] && exit 0
  exit 1
fi

command -v oci >/dev/null || { echo "oci-cli not found: install + run 'oci setup config'"; exit 1; }
command -v jq >/dev/null || { echo "jq not found"; exit 1; }
[ -f "$OCI_SSH_KEY" ] || { echo "SSH key not found: $OCI_SSH_KEY (set OCI_SSH_KEY)"; exit 1; }

echo "⚡ LightSpeed OCI Always-Free Provisioning"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

for region_arg in "$@"; do
  REGION="${REGION_MAP[$region_arg]:-$region_arg}"
  [ -z "$COMPARTMENT" ] && COMPARTMENT=$(tenancy_ocid)
  [ -n "$COMPARTMENT" ] && [ "$COMPARTMENT" != "None" ] || { echo "Cannot resolve compartment; set OCI_COMPARTMENT_OCID"; exit 1; }

  node_name="proxy-${region_arg}"
  echo -e "\n${CYAN}Provisioning $node_name ($REGION)...${NC}"

  subnet=$(ensure_network)
  instance=$(launch "$node_name" "$region_arg" "$subnet") || { echo -e "${RED}❌ $node_name failed to launch${NC}"; continue; }

  ip=$(oc compute instance list-vnics -c "$COMPARTMENT" --instance-id "$instance" --query 'data[0]."public-ip"' --raw-output 2>/dev/null)
  echo -e "  ${GREEN}✅ $node_name → $ip${NC}"
  echo "  The instance self-installs the proxy via cloud-init (~2 min)."
done

echo ""
echo "Done. Verify each relay: curl http://<ip>:8080/health"
