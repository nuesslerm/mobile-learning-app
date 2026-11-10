# ===========================================================================
# Compartment quota — a PREVENTIVE hard cap, layered under budget.tf (alerts)
# and budget-guard.tf (reactive stop/terminate). Quotas are enforced by OCI
# itself at request time: an attempt to provision beyond what's allowed here
# gets rejected before anything is created, regardless of what Terraform code
# or an agent asks for. This is the strongest of the three layers — it can't
# be bypassed by a bug in main.tf's sizing variables the way the other two
# (which react to spend, not to the request itself) can.
#
# Scoped `in tenancy`, NOT `in compartment id var.compartment_ocid` — this
# was originally compartment-scoped, which left a real gap: the credential
# actually used for all Terraform/CLI work here is a personal admin account
# (confirmed via Audit's `principal-name` on live API calls), not a
# narrowly-scoped service user, so it has tenancy-wide permissions and could
# have created paid resources in the root compartment (or any other
# compartment) completely unrestricted by a quota pinned to just this one
# compartment. `in tenancy` closes that — verified this tenancy has no
# other resource-bearing compartment that this cap could unexpectedly
# disrupt (only the root and this repo's own compartment exist), and
# compute/storage quotas don't touch IAM/identity resources (a different
# resource family) even where those live at the root.
#
# Pattern: `zero <family> quotas` (block everything in that family), then
# `set <family> quota <name> to <N>` to allowlist exactly the Always-Free-
# eligible resources this repo actually uses, capped at Oracle's CURRENT
# Always-Free ceiling (halved from 4 OCPU/24GB to 2 OCPU/12GB on 2026-06-15 —
# see docs/oracle-cloud-agent-provisioning.md). Verified against the live
# tenancy's real quota/limit names (`oci limits definition list
# --service-name compute`) rather than guessed — `standard-a1-core-count`
# and `standard-a1-memory-count` are AD-scoped (would allow the cap PER AD,
# multiplying in a multi-AD region); the `-regional-count` variants used
# below are REGION-scoped, matching how Oracle actually allocates the
# Always-Free A1 pool (once regionally, not per AD).
#
# `standard-e2-micro-core-count` and block-storage's `total-storage-gb` have
# no regional variant — they're AD-scoped, so this cap is exact only for a
# single-AD region (true for this repo's eu-frankfurt-1 today); a multi-AD
# region would allow the cap PER AD instead of once regionally. Not worth
# solving for a shape/limit this repo doesn't currently target multi-AD.
# ===========================================================================

variable "always_free_a1_ocpu_cap" {
  description = "Hard regional cap on total A1.Flex OCPUs in the compartment. Matches var.vm_ocpus, which now takes the whole pool in one instance — raise both together if Oracle's allowance ever changes again. Unchanged by the light+heavy -> single-VM consolidation: the cap was always pool-wide, not per-instance, which is exactly why consolidating needed no quota edit."
  type        = number
  default     = 2
}

variable "always_free_a1_memory_cap_gb" {
  description = "Hard regional cap on total A1.Flex memory (GB) in the compartment. Matches var.vm_memory_gbs."
  type        = number
  default     = 12
}

variable "always_free_block_storage_cap_gb" {
  description = "Hard cap on total block/boot volume storage (GB) in the compartment — Always Free's block storage ceiling. Split today as var.boot_volume_gbs (150, the main VM) + var.dummy_boot_volume_gbs (50, the e2e/rescue box) = exactly 200, so raising either without lowering the other will be rejected by OCI at request time."
  type        = number
  default     = 200
}

variable "always_free_object_storage_cap_gb" {
  description = "Hard cap on total Object Storage (GB) in the tenancy — Always Free's allowance is 20GB, the whole allowance (20e9 bytes is under 20 GiB, and the quota stops writes at the cap, so it cannot bill). Every bucket — build cache, verification artifacts, goldens, pnpm cache, tfstate — counts against it."
  type        = number
  default     = 20
}

resource "oci_limits_quota" "always_free_only" {
  compartment_id = var.tenancy_ocid
  name           = "${var.name_prefix}-always-free-only"
  description    = "Blocks provisioning anything beyond the Always-Free-eligible A1.Flex + E2.1.Micro compute pool and 200GB block storage ANYWHERE IN THE TENANCY (root + every compartment) — a preventive cap enforced by OCI itself, independent of budget.tf/budget-guard.tf."

  statements = [
    # Compute (fixed BM/VM shape counts) — block everything, then allowlist
    # exactly the fixed shape this repo actually uses:
    # dummy-test-infra.tf's E2.1.Micro e2e/rescue box (a SEPARATE Always-Free
    # pool of 2 instances, unrelated to the A1.Flex allowance below). Left at
    # zero until 2026-08-15, this silently blocked ever creating that
    # instance (`LaunchInstance` failed with 400-QuotaExceeded on
    # vm-standard-e2-1-micro-count, confirmed live) -- the comment here used
    # to claim "not used at all by this repo", which stopped being true the
    # moment dummy-test-infra.tf was added.
    "zero compute quotas in tenancy",
    "set compute quota vm-standard-e2-1-micro-count to 2 in tenancy",

    # Compute cores — block everything, then allow exactly the two
    # Always-Free shape families this repo uses, at their current ceiling.
    "zero compute-core quotas in tenancy",
    "set compute-core quota standard-a1-core-regional-count to ${var.always_free_a1_ocpu_cap} in tenancy",
    # Also allowlist the AD-scoped variant at the same cap (2026-08-02 fix):
    # OCI's LaunchInstance check evaluates this one too, even for a
    # region-pooled shape -- leaving it at the "zero" default accidentally
    # blocked recreating an EXISTING A1.Flex instance (destroy+apply), not
    # just excess ones. The regional quota above stays the true binding
    # ceiling across all ADs, so this doesn't reopen a multi-AD loophole.
    "set compute-core quota standard-a1-core-count to ${var.always_free_a1_ocpu_cap} in tenancy",
    "set compute-core quota standard-e2-micro-core-count to 2 in tenancy",

    # Compute memory — same pattern, A1.Flex only (E2.1.Micro is fixed-size,
    # no memory quota lever).
    "zero compute-memory quotas in tenancy",
    "set compute-memory quota standard-a1-memory-regional-count to ${var.always_free_a1_memory_cap_gb} in tenancy",
    "set compute-memory quota standard-a1-memory-count to ${var.always_free_a1_memory_cap_gb} in tenancy",

    # Block storage — cap total volume size at the Always-Free ceiling.
    "zero block-storage quotas in tenancy",
    "set block-storage quota total-storage-gb to ${var.always_free_block_storage_cap_gb} in tenancy",

    # Object storage — verified against the live tenancy (`oci limits
    # definition list --service-name object-storage`): `storage-bytes` is a
    # real limit name (region-scoped), defaulting to Oracle's own
    # effectively-unbounded ceiling (int64-max) rather than the Always-Free
    # 20GB, since Always-Free for Object Storage is a BILLING allowance, not
    # an OCI-enforced quota, by default. `bucket-count` is a valid LIMIT
    # name but rejected by the live API as an invalid QUOTA statement name
    # ("is not a valid quota name for service object-storage") -- Limits and
    # Quotas are related but distinct OCI concepts, and not every limit has
    # a corresponding quota-DSL lever; confirmed live rather than assumed,
    # so it's left out rather than guessed at further. The value is a plain
    # byte count with NO unit suffix at all -- confirmed live (both "NGB"
    # and "N GB" produced "Syntax error starting with 'GB'/'NGB'"; Oracle's
    # own docs examples use a raw decimal integer, e.g. "10000000000").
    "zero object-storage quotas in tenancy",
    "set object-storage quota storage-bytes to ${var.always_free_object_storage_cap_gb * 1000000000} in tenancy",
  ]
}
