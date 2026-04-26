# ⚠ LEGACY: Oracle Cloud Infrastructure Terraform

> **Status:** Decommissioned 2026-02-23. Kept for reference/recovery only.
>
> **Active deployment:** see [`infra/scripts/`](../scripts/) for current Vultr tooling.

## What happened

LightSpeed originally targeted Oracle Cloud Always Free (ARM Ampere A1) for zero-cost proxy hosting.
During WF-002 Step 1, OCI ARM instances in `us-sanjose-1` returned "Out of host capacity" errors
consistently. We deployed VM.Standard.E2.1.Micro (AMD) as a workaround (D-001), but that node
had worse Asia-Pacific peering than Vultr.

**Decision D-005** (2026-02-23): Full pivot to Vultr. Key reasons:
- Vultr vc2-1c-1gb: $6/mo, $300 new-account credit = **60+ months free per node**
- 206ms Bangkok→LA vs 213ms Bangkok→OCI San Jose — 7ms better peering
- Always-available capacity (no ARM "out of host" errors)
- Simpler deployment: native binary + systemd, no cloud-init complexity

## What's in here

```
infra/terraform/
├── versions.tf             # Provider requirements (oracle/oci ~> 5.0)
├── provider.tf             # OCI tenancy + region config
├── variables.tf            # OCI auth + region variables
├── networking.tf           # VCN, subnets, security lists
├── instances.tf            # ARM Ampere A1 (or E2.1.Micro fallback) instances
├── outputs.tf              # Public IPs, proxy mesh config JSON
├── terraform.tfvars.example # Example credentials file (no secrets)
└── templates/              # Cloud-init: proxy.toml, systemd unit, fail2ban
```

## If you want to revive OCI

1. Copy `terraform.tfvars.example` → `terraform.tfvars` and fill in your OCI credentials
2. Run `terraform init` (downloads oracle/oci provider)
3. Run `terraform plan` — verify all resources are Always Free eligible
4. Run `terraform apply` with human approval
5. Note: OCI ARM capacity varies by region and time of day. Retry during off-peak hours (early UTC AM).

## Why the Terraform state is committed

`terraform.tfstate` and `terraform.tfstate.backup` are gitignored. The `.terraform/` provider
download directory is also gitignored. Only the configuration files and example tfvars
are tracked.

## Current active infrastructure

See [`infra/scripts/provision-vultr.sh`](../scripts/provision-vultr.sh) and
[`infra/scripts/setup-new-node.sh`](../scripts/setup-new-node.sh) for current tooling.
