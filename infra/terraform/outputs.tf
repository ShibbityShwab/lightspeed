# ──────────────────────────────────────────────────────────────
# LightSpeed Proxy — Terraform Outputs
# ──────────────────────────────────────────────────────────────

output "proxy_nodes" {
  description = "Map of deployed proxy nodes with connection details"
  value = {
    for k, v in var.regions : k => {
      node_id    = v.node_id
      region     = v.oci_region
      public_ip  = oci_core_instance.proxy[k].public_ip
      private_ip = oci_core_instance.proxy[k].private_ip
      data_addr  = "${oci_core_instance.proxy[k].public_ip}:${var.proxy_data_port}"
      ctrl_addr  = "${oci_core_instance.proxy[k].public_ip}:${var.proxy_control_port}"
      health_url = "http://${oci_core_instance.proxy[k].public_ip}:${var.proxy_health_port}/health"
    }
  }
}

output "ssh_private_key" {
  description = "SSH private key for accessing proxy nodes (sensitive)"
  value       = tls_private_key.deploy.private_key_openssh
  sensitive   = true
}

output "ssh_commands" {
  description = "SSH commands to connect to each proxy node"
  value = {
    for k, v in var.regions : k =>
    "ssh -i lightspeed_deploy_key opc@${oci_core_instance.proxy[k].public_ip}"
  }
}

output "proxy_mesh_config" {
  description = "JSON config for the LightSpeed client proxy list"
  value = jsonencode({
    proxies = [
      for k, v in var.regions : {
        node_id  = v.node_id
        region   = k
        addr     = "${oci_core_instance.proxy[k].public_ip}:${var.proxy_data_port}"
        ctrl     = "${oci_core_instance.proxy[k].public_ip}:${var.proxy_control_port}"
        health   = "http://${oci_core_instance.proxy[k].public_ip}:${var.proxy_health_port}/health"
        priority = 1
      }
    ]
  })
}

output "health_check_urls" {
  description = "Health check URLs for monitoring"
  value = [
    for k, v in var.regions :
    "http://${oci_core_instance.proxy[k].public_ip}:${var.proxy_health_port}/health"
  ]
}

# ── Save SSH key to local file ───────────────────────────────

resource "local_file" "ssh_private_key" {
  content         = tls_private_key.deploy.private_key_openssh
  filename        = "${path.module}/lightspeed_deploy_key"
  file_permission = "0600"
}

resource "local_file" "ssh_public_key" {
  content         = tls_private_key.deploy.public_key_openssh
  filename        = "${path.module}/lightspeed_deploy_key.pub"
  file_permission = "0644"
}
