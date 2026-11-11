# ===========================================================================
# Data sources — availability domain + latest Ubuntu aarch64 image
# ===========================================================================
data "oci_identity_availability_domains" "ads" {
  compartment_id = var.tenancy_ocid
}

# Latest Canonical Ubuntu image that supports the A1.Flex (ARM) shape.
data "oci_core_images" "ubuntu_arm" {
  compartment_id           = var.compartment_ocid
  operating_system         = "Canonical Ubuntu"
  operating_system_version = var.ubuntu_os_version
  shape                    = var.instance_shape
  sort_by                  = "TIMECREATED"
  sort_order               = "DESC"
}

locals {
  # A1 free-tier capacity is per-AD; if `apply` fails with an "Out of host
  # capacity" error, retry with -var availability_domain_index=1 or 2 (see
  # scripts/retry-apply.sh).
  availability_domain = data.oci_identity_availability_domains.ads.availability_domains[var.availability_domain_index].name
  image_id            = data.oci_core_images.ubuntu_arm.images[0].id

  # cloud-init: install Docker + Node 22 + the bws CLI, and (if provided) seed
  # each app dir's .env.local with the Bitwarden bootstrap creds. Every app
  # this repo deploys now lives on the single VM: the two real infra services
  # (vm-gateway fronting vm-ops-api) plus all four backend apps that used to be
  # split across light/heavy.
  #
  # The former light_apps/heavy_apps pair, its `_assert_no_app_overlap` guard,
  # and vm-ops-api's per-host branch all existed to stop one app being declared
  # on two hosts at once (the 2026-08-09 Telegram-bot incident: a
  # Telegram long-polling bot allows exactly one live `getUpdates` consumer per
  # token, two copies crash-looped for ~3 hours). With one host that failure is
  # structurally impossible, so the overlap guard is gone rather than kept as
  # dead code. vm-apps.json is the list, with where each app's compose file lives; cloud-init provisions every entry in
  # order (vm-ops-api first) with the secret sets of wf's own groups.json. vm-ops-api's KNOWN_APPS (src/domain/apps.ts)
  # must still agree with it.
  vm_app_entries = jsondecode(file("${path.module}/vm-apps.json"))
  vm_apps        = [for app in local.vm_app_entries : app.name]

  # A VM created without the Bitwarden bootstrap token is a SILENT DUD: cloud-init
  # writes each app's .env.local with no BWS_ADMIN_ACCESS_TOKEN, and
  # provision-apps.js then hits its own `if (!bootstrapEnv.BWS_DEPLOY_SECRETS_
  # READ_ACCESS_TOKEN) process.exit(1)` and aborts before provisioning ANYTHING.
  # The instance boots, reports RUNNING, and serves nothing -- no containers, no
  # tailnet devices, no logs. Confirmed live 2026-08-11: `wf vm creds
  # sync` deliberately does not write this variable (it manages only the
  # OCI block), so the documented "sync-creds then apply" path produces exactly
  # that dud, and the failure is invisible until you notice ~20 minutes later
  # that nothing came up. Fail at plan time instead.
  #
  # Escape hatch preserved rather than removed: variables.tf documents leaving the
  # token blank and writing .env.local by hand on the VM. That needs SSH, which a
  # sandboxed agent session does not have, so it must be opted into explicitly.
  _assert_provisionable = var.bws_read_access_token != "" || var.allow_unprovisioned_vm ? true : tobool("bws_read_access_token is empty, so cloud-init cannot resolve ANY secret and provision-apps.js will abort -- the VM would boot and serve nothing. Set bws_read_access_token (see terraform.tfvars.example), or set allow_unprovisioned_vm = true if you really intend to write each app's .env.local by hand over SSH.")

  # The single host's label, threaded into cloud-init and from there into each
  # app's .env as HOST_LABEL. It ends up in tailscale hostnames
  # (vm-gateway-${host_label}, vm-ops-api-${host_label}) and therefore in the
  # gateway's public Funnel URL, so changing it is a user-visible rename.
  # "main" matches oci_core_vcn.main rather than inheriting "light", which
  # described a 1-OCPU half of a pre-2026-06-15 4-OCPU pool and would now be
  # actively wrong for a VM holding the entire allowance.
  host_label = "main"

  # base64GZIP, not base64encode: OCI caps instance metadata at 32000 bytes, and
  # the plain-base64 rendering of this template had grown to ~31.6KB -- 386
  # bytes of headroom, so any comment added to cloud-init.sh.tftpl silently
  # broke instance creation, discoverable only the next time someone recreated
  # a VM (confirmed live 2026-08-11: "Metadata size is 34361 bytes and cannot be
  # larger than 32000"). cloud-init detects the gzip magic bytes and
  # decompresses user-data itself, so this is transparent to the script; it
  # takes the rendering to ~12.5KB.
  # The bws version, read from the repo's own mise.toml at APPLY time rather
  # than discovered on the VM at boot. mise.toml is the single pin every other
  # surface already uses (clis/env-bootstrap's mise step, the agent-base image),
  # and reading it here means the VM needs no network call and no GitHub
  # credential to learn its version.
  #
  # Deliberately not fetched from raw.githubusercontent at boot, which was the
  # first attempt: this repo is PRIVATE, so an unauthenticated raw fetch returns
  # 404 and the step would have silently resolved an empty version on every
  # single boot -- the same silent-skip failure being removed here, reintroduced
  # in a new place. Verified by probing the real URL rather than assumed.
  #
  # regex() rather than a hand-maintained variable, so bumping mise.toml is the
  # only edit needed; it throws at plan time if the pin ever stops matching,
  # which is the loud failure a boot-time discovery could not give us.
  bws_tag = regex("bws-v[0-9]+\\.[0-9]+\\.[0-9]+", file("${path.module}/../../mise.toml"))

  user_data_main = base64gzip(templatefile("${path.module}/cloud-init.sh.tftpl", {
    bws_read_access_token = var.bws_read_access_token
    bws_server_url        = var.bws_server_url
    git_user_name         = var.git_user_name
    git_user_email        = var.git_user_email
    apps                  = join(" ", local.vm_apps)
    host_label            = local.host_label
    provision_ref         = var.provision_ref
    bws_tag               = local.bws_tag
    # jsonencode then a JSON.parse on the other side, rather than interpolating a
    # JS object literal: the value is attacker-irrelevant but agent-authored, and
    # a bad tag would otherwise be a syntax error inside provision-apps.js that
    # only surfaces on a VM boot with no console access.
    image_tag_overrides_json = jsonencode(var.image_tag_overrides)
    vm_apps_json             = jsonencode(local.vm_app_entries)
    secret_groups_json       = jsonencode(jsondecode(file("${path.module}/../../clis/secrets/src/secrets/groups.json")))
    # where cloud-init finds the VM state repository (the instance principal's rclone remote, vm-state.tf)
    vm_state_namespace = data.oci_objectstorage_namespace.ns.namespace
    vm_state_bucket    = "${var.name_prefix}-vm-state"
    vm_state_region    = var.region
  }))
}

# ===========================================================================
# Network — one VCN, one public subnet, IGW, route table, SSH-only ingress
# ===========================================================================
resource "oci_core_vcn" "main" {
  compartment_id = var.compartment_ocid
  display_name   = "${var.name_prefix}-vcn"
  cidr_blocks    = ["10.0.0.0/16"]
  dns_label      = "mlvcn"
}

resource "oci_core_internet_gateway" "igw" {
  compartment_id = var.compartment_ocid
  vcn_id         = oci_core_vcn.main.id
  display_name   = "${var.name_prefix}-igw"
}

resource "oci_core_route_table" "public" {
  compartment_id = var.compartment_ocid
  vcn_id         = oci_core_vcn.main.id
  display_name   = "${var.name_prefix}-rt"

  route_rules {
    destination       = "0.0.0.0/0"
    network_entity_id = oci_core_internet_gateway.igw.id
  }
}

resource "oci_core_security_list" "public" {
  compartment_id = var.compartment_ocid
  vcn_id         = oci_core_vcn.main.id
  display_name   = "${var.name_prefix}-sl"

  # All egress (the tailscale sidecars + docker pulls need outbound).
  egress_security_rules {
    destination = "0.0.0.0/0"
    protocol    = "all"
  }

  # Inbound SSH only. Everything else (app traffic) rides Tailscale, so no
  # public app ports are opened.
  ingress_security_rules {
    protocol = "6" # TCP
    source   = var.ssh_ingress_cidr
    tcp_options {
      min = 22
      max = 22
    }
  }

  # 80/443 for vm-ops-api (see vm-ops-api/README.md) — a small
  # bearer-token-gated HTTPS PoC letting a sandboxed agent (no SSH/raw-TCP
  # egress) drive/verify VM state without a Docker Hub round-trip. Open to
  # 0.0.0.0/0 deliberately: the calling agent has no fixed source IP to
  # allowlist (it's behind a shared, rotating egress proxy), so the actual
  # access control is the bearer token, not this CIDR. 80 is ACME HTTP-01
  # challenge traffic only (Caddy auto-redirects everything else to 443).
  ingress_security_rules {
    protocol = "6" # TCP
    source   = "0.0.0.0/0"
    tcp_options {
      min = 80
      max = 80
    }
  }
  ingress_security_rules {
    protocol = "6" # TCP
    source   = "0.0.0.0/0"
    tcp_options {
      min = 443
      max = 443
    }
  }
}

resource "oci_core_subnet" "public" {
  compartment_id             = var.compartment_ocid
  vcn_id                     = oci_core_vcn.main.id
  display_name               = "${var.name_prefix}-subnet"
  cidr_block                 = "10.0.1.0/24"
  route_table_id             = oci_core_route_table.public.id
  security_list_ids          = [oci_core_security_list.public.id]
  prohibit_public_ip_on_vnic = false
  dns_label                  = "mlsubnet"
}

# ===========================================================================
# Compute — the single VM (Ampere A1 / ARM64), holding the entire current
# Always-Free A1 allowance (2 OCPU / 12 GB). Replaced the former light+heavy
# pair; see variables.tf's sizing block for why the split was dropped.
# ===========================================================================
resource "oci_core_instance" "main" {
  # the restore in cloud-init reads the state bucket with the instance principal these two grant
  depends_on = [oci_identity_policy.vm_state]

  compartment_id      = var.compartment_ocid
  availability_domain = local.availability_domain
  display_name        = "${var.name_prefix}-main"
  shape               = var.instance_shape

  # E2.1.Micro (the x86 Always-Free fallback shape) is fixed-size and
  # rejects shape_config outright — only pass it for a *.Flex shape like
  # A1.Flex. See scripts/retry-apply.sh's --allow-e2-micro-fallback.
  dynamic "shape_config" {
    for_each = endswith(var.instance_shape, ".Flex") ? [1] : []
    content {
      ocpus         = var.vm_ocpus
      memory_in_gbs = var.vm_memory_gbs
    }
  }

  create_vnic_details {
    subnet_id = oci_core_subnet.public.id
    # Ephemeral, not a RESERVED oci_core_public_ip. A reserved IP was tried and
    # removed: it only survived `apply -replace`, NOT `destroy -target` of this
    # instance (the IP resource hung off the instance's own private IP), so its
    # stability guarantee had a silent exception -- and the address still churned
    # three times in one session. It also outlives the instance, making it
    # orphanable exactly like the stray boot volume that was found eating a
    # quarter of the block-storage quota.
    #
    # Nothing needs a stable ADDRESS anyway: every service is reached over
    # Tailscale by MagicDNS name, which survives recreation for free, and all
    # four apps are redeployable via vm-ops-api over the tailnet with no SSH at
    # all. The public IP is now only an SSH convenience.
    assign_public_ip = true
    hostname_label   = local.host_label
  }

  source_details {
    source_type             = "image"
    source_id               = local.image_id
    boot_volume_size_in_gbs = var.boot_volume_gbs
  }

  metadata = {
    ssh_authorized_keys = join("\n", compact([trimspace(file(var.ssh_public_key_path)), trimspace(var.break_glass_ssh_public_key)]))
    user_data           = local.user_data_main
  }

  # cloud-init only runs on first boot -- an edit to cloud-init.sh.tftpl (or
  # any of local.user_data_main's other template inputs) after that point should
  # never force a full destroy+recreate of an already-provisioned, live VM.
  # Without this, `terraform apply` silently plans to replace the production
  # instance (losing tailscale device identity, any other non-compose-managed
  # volume state, and possibly the reserved public IP) the moment
  # cloud-init.sh.tftpl gets a routine bugfix -- confirmed live: three merged
  # cloud-init fixes after the instance's creation date each produced exactly
  # this "must be replaced" plan with zero code changes of their own intent to
  # touch the instance. A real, intentional re-image should use
  # `terraform apply -replace=oci_core_instance.main` explicitly instead of
  # relying on incidental metadata drift.
  #
  # NOTE this also means var.break_glass_ssh_public_key only lands on a
  # CREATE -- adding a Termius key to a running VM needs the -replace above.
  #
  # source_id is ignored for the same reason. local.image_id is "the newest Canonical Ubuntu image", so every image Canonical
  # (and OCI) publishes would otherwise make the next plan say "must be replaced" on the live VM, whatever the change was about
  # (this is the drift the 2026-11 recreate was planned around). An image bump is an intentional re-image, never a side effect:
  # `wf vm terraform apply --recreate` destroys the instance and creates it again, and a create always takes the CURRENT
  # image id, so the recreate still lands on the newest image. `wf vm terraform preflight` plans with -replace, so it shows
  # exactly what that recreate does while a plain plan stays quiet. boot_volume_size_in_gbs and every other attribute still
  # force a replace when they change.
  #
  # State that lives only on the boot volume is NOT preserved by this resource (no preserve_boot_volume, no block volume):
  # the 200 GB Always-Free block storage cap is already used by the 150 GB boot volume plus the 50 GB dummy box, so a kept
  # volume beside the new one is QuotaExceeded. docs/runbooks/vm-recreate.md lists what is lost and how each piece comes back.
  lifecycle {
    ignore_changes = [metadata, source_details[0].source_id]
  }
}
