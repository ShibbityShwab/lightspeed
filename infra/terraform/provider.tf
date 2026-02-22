# ──────────────────────────────────────────────────────────────
# LightSpeed Proxy — OCI Provider Configuration
# ──────────────────────────────────────────────────────────────
#
# Uses a single OCI provider targeting the home region.
# Per-region resources are created using provider aliases
# configured dynamically via the regions variable.
# ──────────────────────────────────────────────────────────────

provider "oci" {
  tenancy_ocid     = var.tenancy_ocid
  user_ocid        = var.user_ocid
  fingerprint      = var.fingerprint
  private_key_path = var.private_key_path
  region           = var.home_region
}
