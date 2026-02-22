# ──────────────────────────────────────────────────────────────
# LightSpeed Proxy — Compute Instances
#
# ARM Ampere A1 (Always Free): up to 4 OCPUs / 24GB total.
# Default: 3 nodes × (1 OCPU, 6GB) = 3 OCPUs, 18GB used.
# Remaining: 1 OCPU, 6GB for future use.
# ──────────────────────────────────────────────────────────────

# ── SSH Key ──────────────────────────────────────────────────

resource "tls_private_key" "deploy" {
  algorithm = "ED25519"
}

# ── Latest Oracle Linux 9 ARM Image ─────────────────────────

data "oci_core_images" "oracle_linux" {
  for_each = var.regions

  compartment_id           = local.compartment_id
  operating_system         = "Oracle Linux"
  operating_system_version = "9"
  shape                    = var.instance_shape
  sort_by                  = "TIMECREATED"
  sort_order               = "DESC"

  filter {
    name   = "display_name"
    values = ["^Oracle-Linux-9\\..*-aarch64-.*$"]
    regex  = true
  }
}

locals {
  # Use provided image ID or auto-detect latest per region
  image_ids = {
    for k, v in var.regions : k => (
      var.os_image_id != "" ? var.os_image_id : data.oci_core_images.oracle_linux[k].images[0].id
    )
  }
}

# ── Cloud-Init User Data ────────────────────────────────────

data "cloudinit_config" "proxy" {
  for_each = var.regions

  gzip          = true
  base64_encode = true

  part {
    content_type = "text/cloud-config"
    content = yamlencode({
      package_update  = true
      package_upgrade = true

      packages = [
        "docker-ce",
        "docker-ce-cli",
        "containerd.io",
        "fail2ban",
        "firewalld",
      ]

      # Add Docker CE repo before installing
      yum_repos = {
        docker-ce-stable = {
          name     = "Docker CE Stable"
          baseurl  = "https://download.docker.com/linux/centos/$releasever/$basearch/stable"
          enabled  = true
          gpgcheck = true
          gpgkey   = "https://download.docker.com/linux/centos/gpg"
        }
      }

      write_files = [
        {
          path        = "/etc/lightspeed/proxy.toml"
          permissions = "0644"
          content = templatefile("${path.module}/templates/proxy.toml.tpl", {
            node_id                   = each.value.node_id
            region                    = each.key
            max_clients               = var.proxy_max_clients
            require_auth              = false # Dev mode for initial deployment
            max_amplification_ratio   = 2.0
            max_destinations_per_window = 10
            ban_duration_secs         = 3600
          })
        },
        {
          path        = "/etc/systemd/system/lightspeed-proxy.service"
          permissions = "0644"
          content     = file("${path.module}/templates/lightspeed-proxy.service")
        },
        {
          path        = "/etc/fail2ban/jail.d/lightspeed.conf"
          permissions = "0644"
          content     = file("${path.module}/templates/fail2ban-lightspeed.conf")
        },
        {
          path        = "/etc/lightspeed/deploy.sh"
          permissions = "0755"
          content     = file("${path.module}/templates/deploy.sh")
        },
      ]

      runcmd = [
        # Enable and start Docker
        "systemctl enable --now docker",
        # Enable and start fail2ban
        "systemctl enable --now fail2ban",
        # Configure firewalld
        "systemctl enable --now firewalld",
        "firewall-cmd --permanent --add-port=${var.proxy_data_port}/udp",
        "firewall-cmd --permanent --add-port=${var.proxy_control_port}/udp",
        "firewall-cmd --permanent --add-port=${var.proxy_health_port}/tcp",
        "firewall-cmd --permanent --add-port=22/tcp",
        "firewall-cmd --reload",
        # Kernel tuning for UDP performance
        "sysctl -w net.core.rmem_max=26214400",
        "sysctl -w net.core.wmem_max=26214400",
        "sysctl -w net.core.rmem_default=1048576",
        "sysctl -w net.core.wmem_default=1048576",
        "sysctl -w net.core.netdev_max_backlog=50000",
        "echo 'net.core.rmem_max=26214400' >> /etc/sysctl.d/99-lightspeed.conf",
        "echo 'net.core.wmem_max=26214400' >> /etc/sysctl.d/99-lightspeed.conf",
        "echo 'net.core.rmem_default=1048576' >> /etc/sysctl.d/99-lightspeed.conf",
        "echo 'net.core.wmem_default=1048576' >> /etc/sysctl.d/99-lightspeed.conf",
        "echo 'net.core.netdev_max_backlog=50000' >> /etc/sysctl.d/99-lightspeed.conf",
        # Pull and start the proxy (will fail until image is pushed — that's ok)
        "docker pull ghcr.io/shibbityshwab/lightspeed-proxy:latest || true",
        "bash /etc/lightspeed/deploy.sh || true",
      ]
    })
  }
}

# ── Compute Instances ────────────────────────────────────────

resource "oci_core_instance" "proxy" {
  for_each = var.regions

  compartment_id      = local.compartment_id
  availability_domain = data.oci_identity_availability_domains.ads[each.key].availability_domains[0].name
  display_name        = each.value.display_name
  shape               = var.instance_shape

  shape_config {
    ocpus         = each.value.shape_ocpus
    memory_in_gbs = each.value.shape_memory
  }

  source_details {
    source_type             = "image"
    source_id               = local.image_ids[each.key]
    boot_volume_size_in_gbs = var.boot_volume_size_gb
  }

  create_vnic_details {
    subnet_id        = oci_core_subnet.lightspeed[each.key].id
    assign_public_ip = true
    display_name     = "${each.value.display_name}-vnic"
    hostname_label   = "proxy"
  }

  metadata = {
    ssh_authorized_keys = tls_private_key.deploy.public_key_openssh
    user_data           = data.cloudinit_config.proxy[each.key].rendered
  }

  freeform_tags = var.tags

  # Prevent accidental destruction
  lifecycle {
    prevent_destroy = false # Set to true after initial deployment
  }
}

# ── Availability Domains ─────────────────────────────────────

data "oci_identity_availability_domains" "ads" {
  for_each       = var.regions
  compartment_id = local.compartment_id
}
