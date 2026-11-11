# ---------------------------------------------------------------------------
# OCI account / auth (from your API signing key — see README "Getting the OCIDs")
# ---------------------------------------------------------------------------
variable "tenancy_ocid" {
  description = "Tenancy OCID (Profile menu → Tenancy, or ~/.oci/config)."
  type        = string
}

variable "user_ocid" {
  description = "User OCID of the account whose API key you generated."
  type        = string
}

variable "api_key_fingerprint" {
  description = "Fingerprint of the uploaded API signing key."
  type        = string
}

variable "api_private_key_path" {
  description = "Local path to the API signing private key (PEM)."
  type        = string
}

variable "api_private_key_passphrase" {
  description = "Passphrase for the API signing private key, if it's encrypted. Leave blank for a key generated via the OCI Console's 'Add API key → Generate API key pair' button (unencrypted by default) — only needed if you generated the keypair yourself (e.g. openssl) with a passphrase and uploaded the public half to OCI."
  type        = string
  default     = ""
  sensitive   = true
}

variable "region" {
  description = "Home/region identifier, e.g. eu-frankfurt-1, us-ashburn-1."
  type        = string
}

variable "compartment_ocid" {
  description = "Compartment to create everything in (root tenancy OCID is fine for a personal account)."
  type        = string
}

# ---------------------------------------------------------------------------
# Access control
# ---------------------------------------------------------------------------
variable "ssh_public_key_path" {
  description = "Path to the DEDICATED deploy keypair's PUBLIC half (the private half becomes the DEPLOY_SSH_KEY GitHub secret). e.g. ~/.ssh/oracle_deploy.pub"
  type        = string
}

variable "ssh_ingress_cidr" {
  description = "CIDR allowed to reach SSH (22). Lock to your IP as \"x.x.x.x/32\" for a real setup; 0.0.0.0/0 is open to the world."
  type        = string
  default     = "0.0.0.0/0"
}

variable "break_glass_ssh_public_key" {
  description = "Optional secondary SSH public key added alongside the dedicated deploy keypair in the VM's ssh_authorized_keys (e.g. a personal client like Termius). Not secret -- public keys are safe to pass via terraform.tfvars (gitignored, kept out of committed code only to avoid churn if rotated). Empty by default so normal applies are unaffected. Only takes effect on instance CREATE (ignore_changes on metadata means an in-place apply won't push this to an already-running instance -- use `terraform apply -replace=oci_core_instance.main` to actually recreate with the new key). Replaces the former per-host light_/heavy_ pair, which existed only because there were two instances."
  type        = string
  default     = ""
}

# ---------------------------------------------------------------------------
# Shapes / sizing — VM.Standard.A1.Flex is the Ampere (ARM64) Always-Free pool.
# Oracle silently halved this allowance on 2026-06-15, from 4 OCPU + 24 GB to
# 2 OCPU + 12 GB total across all A1 instances in a tenancy (no announcement —
# see docs/oracle-cloud-agent-provisioning.md). ONE instance now takes the
# WHOLE pool (2 OCPU / 12 GB) rather than two 1-OCPU/6-GB halves.
#
# WHY ONE VM: the original split was light (1/6) + heavy (3/18) = the old
# 4/24 pool, so the names described a real size difference. After the halving
# both were flattened to an identical 1 OCPU / 6 GB and the names became
# fossils. Nothing else justified the split: the only isolation it bought was
# "which host", since no compose file sets a cpu/memory limit, while it cost a
# duplicated control plane (vm-gateway + vm-ops-api on both hosts), a doubled
# Terraform surface, two capacity placements that both had to succeed, and an
# entire bug class that only exists with two hosts (the 2026-08-09
# Telegram-bot double-provisioning incident, plus vm-gateway's
# TS_HOSTNAME collision). Consolidating is quota-neutral: always-free-quota.tf
# already caps the pool at 2 OCPU / 12 GB regionally, so one 2-OCPU instance
# needs no quota change.
# ---------------------------------------------------------------------------
variable "instance_shape" {
  description = "Compute shape. Keep A1.Flex to stay in the ARM Always-Free pool. scripts/retry-apply.sh can override this to the x86 VM.Standard.E2.1.Micro Always-Free fallback (--allow-e2-micro-fallback) as a last resort when A1 capacity is unavailable everywhere — that shape is fixed at 1/8 OCPU + 1 GB RAM, likely too small to run this repo's docker + node services comfortably; treat it as a stopgap to get *something* reachable, not a real substitute."
  type        = string
  default     = "VM.Standard.A1.Flex"
}

variable "vm_ocpus" {
  description = "OCPUs for the single A1.Flex VM. 2 is the entire current Always-Free A1 pool; always_free_a1_ocpu_cap must not be lower."
  type        = number
  default     = 2
}

variable "vm_memory_gbs" {
  description = "Memory (GB) for the single A1.Flex VM. 12 is the entire current Always-Free A1 pool; always_free_a1_memory_cap_gb must not be lower."
  type        = number
  default     = 12
}

variable "boot_volume_gbs" {
  description = "Boot volume size (GB) for the single A1.Flex VM. Always-Free block storage is 200 GB TOTAL across every volume in the tenancy, and dummy-test-infra.tf's permanent e2e box holds dummy_boot_volume_gbs of it — so this is capped at 200 minus that (150 today). Deliberately NOT shared with the dummy instance any more: both used to read this one variable, so raising it here silently force-replaced the e2e box too (boot_volume_size_in_gbs is ForceNew)."
  type        = number
  default     = 150
}

variable "dummy_boot_volume_gbs" {
  description = "Boot volume size (GB) for dummy-test-infra.tf's permanent E2.1.Micro e2e-test instance. Split out from boot_volume_gbs so resizing the real VM can't force-replace this box (and lose the state-store history pointing at it). Also the break-glass rescue host: attaching the A1 VM's boot volume here as a DATA volume needs no extra block storage, which is why boot_volume_gbs can safely take all remaining headroom."
  type        = number
  default     = 50
}

variable "ubuntu_os_version" {
  description = "Canonical Ubuntu version to look up an aarch64 image for."
  type        = string
  default     = "24.04"
}

variable "name_prefix" {
  description = "Prefix for all resource display names."
  type        = string
  default     = "mobile-learning"
}

variable "availability_domain_index" {
  description = "Index into the region's availability_domains list (0, 1, 2, ...). A1 free-tier capacity is allocated per-AD; when apply fails with an \"Out of host capacity\" error, retry with a different index — see scripts/retry-apply.sh, which cycles this via -var rather than editing main.tf."
  type        = number
  default     = 0
}

# ---------------------------------------------------------------------------
# Bitwarden Secrets Manager bootstrap (baked into each VM's ~/<app>/.env.local by
# cloud-init so `wf secret sync-env --backend bitwarden` can reach the vault).
# These are the ONLY secrets Terraform touches; mark the state sensitive.
# Leave blank to skip — you can write .env.local by hand on the VM instead.
# ---------------------------------------------------------------------------
variable "bws_read_access_token" {
  description = "Bitwarden machine-account access token with project access 'Can read' — the READ-ONLY deploy credential, written to each VM as BWS_ADMIN_ACCESS_TOKEN. Never put the write-capable BWS_ADMIN_ACCESS_TOKEN on a VM: a host running model-generated code should be able to read secrets, not rewrite them. EFFECTIVELY REQUIRED: leaving it blank produces a VM that boots, reports RUNNING, and provisions nothing at all (provision-apps.js aborts on the missing token) — main.tf's _assert_provisionable now refuses to plan rather than letting that happen silently. `wf vm creds sync` does NOT write this, so set it yourself."
  type        = string
  default     = ""
  sensitive   = true
}

variable "allow_unprovisioned_vm" {
  description = "Escape hatch for main.tf's _assert_provisionable: allows creating the VM with an empty bws_read_access_token, i.e. deliberately unprovisioned, on the understanding that you will write each app's .env.local by hand over SSH. Off by default because a sandboxed agent session has no SSH and would just get a host that serves nothing."
  type        = bool
  default     = false
}

# ---------------------------------------------------------------------------
# Git commit identity for the VM (remote-chat-agent commits from worktrees).
# Without a global git identity, the agent's first `git commit` fails with
# "Author identity unknown". Defaults attribute commits to the AGENT rather than
# impersonating you — override if you want your own name/email on them.
# ---------------------------------------------------------------------------
variable "git_user_name" {
  description = "git user.name set globally for the ubuntu user, used by remote-chat-agent's commits."
  type        = string
  default     = "remote-chat-agent"
}

variable "git_user_email" {
  description = "git user.email set globally for the ubuntu user. A @users.noreply.github.com address keeps a real inbox out of public commit metadata."
  type        = string
  default     = "remote-chat-agent@users.noreply.github.com"
}

variable "bws_server_url" {
  description = "Bitwarden region, baked into each VM's .env.local. Defaults to the EU cloud because this repo's account is EU-hosted; set to \"\" for a US-hosted Bitwarden account (bws's own default is the US cloud, and it rejects EU tokens with a bare invalid_client)."
  type        = string
  default     = "https://vault.bitwarden.eu"
}

# ---------------------------------------------------------------------------
# Pre-merge verification levers. Both default to the normal production values,
# so a plain apply behaves exactly as before. They exist because cloud-init used
# to hardcode BOTH the branch it fetched app compose files from ('main') and the
# image tag every app ran ('latest', which only the default branch ever
# publishes) — so a change to either was impossible to test on a real VM without
# merging it first and hoping.
# ---------------------------------------------------------------------------
variable "provision_ref" {
  description = "Git ref cloud-init fetches each app's compose file + .env.defaults from. Defaults to main. Point at a branch to validate a compose-file change on a throwaway recreate before merging."
  type        = string
  default     = "dev"
}

variable "image_tag_overrides" {
  description = "Per-app image tag overrides, e.g. { \"vm-ops-api\" = \"sha-abc1234\" }. Any app not listed uses its compose file's own default (`latest`). Use this to run a branch-built image on the VM; `latest` is only published from the default branch."
  type        = map(string)
  default     = {}
}

# ---------------------------------------------------------------------------
# Autonomous Database (health-report storage) — see autonomous-db.tf.
# ---------------------------------------------------------------------------
variable "adb_admin_password" {
  description = "ADMIN password for the health-reports Autonomous Database. Oracle's rule: 12-30 chars, at least one uppercase, one lowercase, one number, no username substring, no double-quote. Generate with e.g. `openssl rand -base64 24` and adjust for the charset rule; store in Bitwarden (ADB_ADMIN_PASSWORD), never commit. Only used to create/rotate the DB user itself -- ORDS's own HTTPS auth (set up separately by the one-time setup script) is what the frontend/CLI actually authenticate against day to day."
  type        = string
  sensitive   = true
}
