# A small, permanent, Always-Free (VM.Standard.E2.1.Micro -- a separate free
# pool from the main VM's A1.Flex) instance, originally the OCI e2e test target
# (its resource and hostname labels keep that name; renaming forces a recreate).
#
# It is the break-glass rescue host: it is an already-provisioned box with
# docker on a DIFFERENT compute pool, so the main VM's boot volume can be
# detached (while that VM is stopped) and attached HERE as a data volume to
# edit its filesystem. That consumes no extra block storage, which is what lets
# var.boot_volume_gbs take all remaining headroom without stranding the
# no-SSH recovery path.

resource "oci_core_instance" "dummy_infra_ops_e2e" {
  count               = var.rescue_host ? 1 : 0
  compartment_id      = var.compartment_ocid
  availability_domain = local.availability_domain
  display_name        = "${var.name_prefix}-dummy-infra-ops-e2e"
  # E2.1.Micro (x86_64), NOT A1.Flex -- confirmed live that this tenancy's
  # A1.Flex compartment quota has ZERO headroom beyond the 2 OCPU/12GB the main
  # VM now holds in full (the actual enforced quota is stricter than the
  # theoretical 4 OCPU/24GB Always Free ceiling, which Oracle halved on
  # 2026-06-15; `terraform apply` failed cleanly with QuotaExceeded, no
  # orphaned resource -- OCI's own enforcement is the real backstop behind this
  # repo's free-tier guard, and it held). E2.1.Micro is a SEPARATE, unused
  # free-tier pool (2 instances), so this fits without touching the A1
  # headroom at all -- which is equally true whether that pool is held by one
  # instance or two.
  shape = "VM.Standard.E2.1.Micro"

  create_vnic_details {
    subnet_id        = oci_core_subnet.public.id
    assign_public_ip = true
    hostname_label   = "dummy-infra-ops-e2e"
  }

  source_details {
    source_type = "image"
    # E2.1.Micro is x86_64 -- the main VM's local.image_id is ARM64 (A1.Flex)
    # and is rejected outright for this shape ("Shape ... is not valid for
    # image ...", confirmed live). A distinct x86_64 Ubuntu 22.04 image,
    # looked up via `oci compute image list --shape VM.Standard.E2.1.Micro`.
    source_id = "ocid1.image.oc1.eu-frankfurt-1.aaaaaaaa3gouyfpblatpyobx75p322i6yqo5xoynrqavmyorxrm6xkxiu4wq"
    # Its OWN variable, not the main VM's boot_volume_gbs: these shared one
    # variable until now, so raising the real VM's disk silently force-replaced
    # this box too (boot_volume_size_in_gbs is ForceNew), losing the
    # state-store history that e2e runs point at.
    boot_volume_size_in_gbs = var.dummy_boot_volume_gbs
  }

  metadata = {
    ssh_authorized_keys = file(var.ssh_public_key_path)
    # Deliberately minimal, unlike the main VM's cloud-init.sh.tftpl: this box
    # runs no real app, just docker + a trivial compose stack that
    # recoverService/rollbackService can swap tags on.
    user_data = base64encode(<<-EOT
      #!/bin/bash
      set -e
      apt-get update
      apt-get install -y docker.io docker-compose-plugin
      mkdir -p /home/ubuntu/dummy-service
      cat > /home/ubuntu/dummy-service/docker-compose.yml <<'COMPOSE_EOF'
      services:
        app:
          image: busybox:$${IMAGE_TAG:-1.36}
          command: sleep infinity
      COMPOSE_EOF
      echo 'IMAGE_TAG=1.36' > /home/ubuntu/dummy-service/.env
      cd /home/ubuntu/dummy-service && docker compose up -d
    EOT
    )
  }

  # Same rationale as the main VM -- an unrelated cloud-init edit must never
  # silently plan to replace this instance (which would lose whatever
  # e2e-recorded state-store history points at it).
  lifecycle {
    ignore_changes = [metadata]
  }
}

output "dummy_infra_ops_e2e_instance_id" {
  description = "OCID of the dummy e2e-test instance -- read by tests/e2e via `terraform output -json`."
  value       = one(oci_core_instance.dummy_infra_ops_e2e[*].id)
}

output "dummy_infra_ops_e2e_public_ip" {
  value = one(oci_core_instance.dummy_infra_ops_e2e[*].public_ip)
}

variable "rescue_host" {
  description = "Create the E2.1.Micro rescue host. Off: OCI refuses the create in this tenancy (LimitExceeded, 17 Oct 2026), so it would fail every full apply; turn it on only for a boot-volume rescue."
  type        = bool
  default     = false
}
