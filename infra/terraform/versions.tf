# ──────────────────────────────────────────────────────────────
# LightSpeed Proxy — Terraform Provider Requirements
# Oracle Cloud Always Free Tier Infrastructure
#
# ⚠  LEGACY — OCI deployment decommissioned 2026-02-23
# Active infrastructure uses Vultr + native binary + systemd.
# See infra/scripts/ for current deployment tooling.
# See infra/terraform/LEGACY-OCI.md for context.
# ──────────────────────────────────────────────────────────────

terraform {
  required_version = ">= 1.5.0"

  required_providers {
    oci = {
      source  = "oracle/oci"
      version = "~> 5.0"
    }
    tls = {
      source  = "hashicorp/tls"
      version = "~> 4.0"
    }
    cloudinit = {
      source  = "hashicorp/cloudinit"
      version = "~> 2.3"
    }
  }
}
