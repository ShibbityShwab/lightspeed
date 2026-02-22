# ──────────────────────────────────────────────────────────────
# LightSpeed Proxy — Networking (VCN, Subnet, Security Lists)
#
# One VCN + public subnet per region, with security rules that
# allow only the ports LightSpeed needs:
#   - UDP 4434 (data plane)
#   - UDP 4433 (QUIC control plane)
#   - TCP 8080 (health check)
#   - TCP 22   (SSH — restricted CIDR)
# ──────────────────────────────────────────────────────────────

# ── Compartment (use tenancy root for Always Free) ───────────

data "oci_identity_compartment" "tenancy" {
  id = var.tenancy_ocid
}

locals {
  compartment_id = data.oci_identity_compartment.tenancy.id
}

# ── VCN ──────────────────────────────────────────────────────

resource "oci_core_vcn" "lightspeed" {
  for_each = var.regions

  compartment_id = local.compartment_id
  display_name   = "${each.value.display_name}-vcn"
  cidr_blocks    = [var.vcn_cidr]
  dns_label      = "ls${replace(each.key, "-", "")}"

  freeform_tags = var.tags
}

# ── Internet Gateway ─────────────────────────────────────────

resource "oci_core_internet_gateway" "lightspeed" {
  for_each = var.regions

  compartment_id = local.compartment_id
  vcn_id         = oci_core_vcn.lightspeed[each.key].id
  display_name   = "${each.value.display_name}-igw"
  enabled        = true

  freeform_tags = var.tags
}

# ── Route Table ──────────────────────────────────────────────

resource "oci_core_route_table" "lightspeed" {
  for_each = var.regions

  compartment_id = local.compartment_id
  vcn_id         = oci_core_vcn.lightspeed[each.key].id
  display_name   = "${each.value.display_name}-rt"

  route_rules {
    network_entity_id = oci_core_internet_gateway.lightspeed[each.key].id
    destination       = "0.0.0.0/0"
    destination_type  = "CIDR_BLOCK"
  }

  freeform_tags = var.tags
}

# ── Security List ────────────────────────────────────────────

resource "oci_core_security_list" "lightspeed" {
  for_each = var.regions

  compartment_id = local.compartment_id
  vcn_id         = oci_core_vcn.lightspeed[each.key].id
  display_name   = "${each.value.display_name}-sl"

  # ── Egress: allow all outbound ─────────────────────────────
  egress_security_rules {
    protocol    = "all"
    destination = "0.0.0.0/0"
    stateless   = false
  }

  # ── Ingress: UDP data plane (4434) ─────────────────────────
  ingress_security_rules {
    protocol  = "17" # UDP
    source    = "0.0.0.0/0"
    stateless = true

    udp_options {
      min = var.proxy_data_port
      max = var.proxy_data_port
    }
  }

  # ── Ingress: UDP/QUIC control plane (4433) ─────────────────
  ingress_security_rules {
    protocol  = "17" # UDP
    source    = "0.0.0.0/0"
    stateless = true

    udp_options {
      min = var.proxy_control_port
      max = var.proxy_control_port
    }
  }

  # ── Ingress: TCP health check (8080) ───────────────────────
  ingress_security_rules {
    protocol  = "6" # TCP
    source    = "0.0.0.0/0"
    stateless = false

    tcp_options {
      min = var.proxy_health_port
      max = var.proxy_health_port
    }
  }

  # ── Ingress: SSH (22) — restricted ─────────────────────────
  dynamic "ingress_security_rules" {
    for_each = var.ssh_allowed_cidrs
    content {
      protocol  = "6" # TCP
      source    = ingress_security_rules.value
      stateless = false

      tcp_options {
        min = 22
        max = 22
      }
    }
  }

  # ── Ingress: ICMP (ping for diagnostics) ───────────────────
  ingress_security_rules {
    protocol  = "1" # ICMP
    source    = "0.0.0.0/0"
    stateless = false

    icmp_options {
      type = 3 # Destination Unreachable
      code = 4 # Fragmentation Needed
    }
  }

  ingress_security_rules {
    protocol  = "1" # ICMP
    source    = "0.0.0.0/0"
    stateless = false

    icmp_options {
      type = 8 # Echo Request (ping)
    }
  }

  freeform_tags = var.tags
}

# ── Public Subnet ────────────────────────────────────────────

resource "oci_core_subnet" "lightspeed" {
  for_each = var.regions

  compartment_id             = local.compartment_id
  vcn_id                     = oci_core_vcn.lightspeed[each.key].id
  display_name               = "${each.value.display_name}-subnet"
  cidr_block                 = var.subnet_cidr
  route_table_id             = oci_core_route_table.lightspeed[each.key].id
  security_list_ids          = [oci_core_security_list.lightspeed[each.key].id]
  dns_label                  = "proxy"
  prohibit_public_ip_on_vnic = false

  freeform_tags = var.tags
}
