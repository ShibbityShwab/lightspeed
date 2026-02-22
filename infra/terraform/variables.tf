# ──────────────────────────────────────────────────────────────
# LightSpeed Proxy — Input Variables
# ──────────────────────────────────────────────────────────────

# ── OCI Authentication ───────────────────────────────────────

variable "tenancy_ocid" {
  description = "OCID of the OCI tenancy"
  type        = string
}

variable "user_ocid" {
  description = "OCID of the OCI user"
  type        = string
}

variable "fingerprint" {
  description = "Fingerprint of the OCI API signing key"
  type        = string
}

variable "private_key_path" {
  description = "Path to the OCI API private key PEM file"
  type        = string
}

# ── Region Configuration ─────────────────────────────────────

variable "regions" {
  description = "Map of proxy regions to deploy. Key = logical name, value = OCI region identifier."
  type = map(object({
    oci_region    = string
    node_id       = string
    display_name  = string
    shape_ocpus   = number
    shape_memory  = number
  }))
  default = {
    us-east = {
      oci_region    = "us-ashburn-1"
      node_id       = "proxy-us-east"
      display_name  = "lightspeed-us-east"
      shape_ocpus   = 1
      shape_memory  = 6
    }
    eu-west = {
      oci_region    = "eu-frankfurt-1"
      node_id       = "proxy-eu-west"
      display_name  = "lightspeed-eu-west"
      shape_ocpus   = 1
      shape_memory  = 6
    }
    asia-se = {
      oci_region    = "ap-singapore-1"
      node_id       = "proxy-asia-se"
      display_name  = "lightspeed-asia-se"
      shape_ocpus   = 1
      shape_memory  = 6
    }
  }
}

variable "home_region" {
  description = "OCI home region for tenancy-level resources (IAM, etc.)"
  type        = string
  default     = "us-ashburn-1"
}

# ── Instance Configuration ───────────────────────────────────

variable "instance_shape" {
  description = "Compute shape for proxy nodes (Always Free = VM.Standard.A1.Flex)"
  type        = string
  default     = "VM.Standard.A1.Flex"
}

variable "boot_volume_size_gb" {
  description = "Boot volume size in GB (Always Free allows up to 200GB total)"
  type        = number
  default     = 50
}

variable "os_image_id" {
  description = "OCID of the OS image. Leave empty to auto-detect latest Oracle Linux 9 ARM."
  type        = string
  default     = ""
}

# ── Network Configuration ────────────────────────────────────

variable "vcn_cidr" {
  description = "CIDR block for the VCN"
  type        = string
  default     = "10.0.0.0/16"
}

variable "subnet_cidr" {
  description = "CIDR block for the public subnet"
  type        = string
  default     = "10.0.1.0/24"
}

# ── Proxy Configuration ──────────────────────────────────────

variable "proxy_data_port" {
  description = "UDP port for the data plane"
  type        = number
  default     = 4434
}

variable "proxy_control_port" {
  description = "UDP/QUIC port for the control plane"
  type        = number
  default     = 4433
}

variable "proxy_health_port" {
  description = "TCP port for the health check HTTP endpoint"
  type        = number
  default     = 8080
}

variable "proxy_max_clients" {
  description = "Maximum concurrent client tunnels per node"
  type        = number
  default     = 100
}

# ── SSH Access ───────────────────────────────────────────────

variable "ssh_public_key_path" {
  description = "Path to SSH public key for instance access"
  type        = string
  default     = "~/.ssh/id_rsa.pub"
}

variable "ssh_allowed_cidrs" {
  description = "CIDR blocks allowed to SSH into proxy nodes"
  type        = list(string)
  default     = ["0.0.0.0/0"]
}

# ── Tags ─────────────────────────────────────────────────────

variable "tags" {
  description = "Freeform tags to apply to all resources"
  type        = map(string)
  default = {
    project     = "lightspeed"
    environment = "production"
    managed_by  = "terraform"
  }
}
