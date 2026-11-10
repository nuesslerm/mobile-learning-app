# ===========================================================================
# Object Storage (Always Free) -- the VM's sensitive state, backed up so a recreate loses none of it (`wf vm state`,
# clis/deploy/src/vm_state, docs/runbooks/vm-recreate.md "State backup"). Block storage is at its 200 GB cap, so a state volume cannot
# outlive the boot volume; this bucket is where it lives meanwhile.
#
# The bucket holds one restic repository (config, keys, index, snapshots, data packs): incremental, encrypted with the restic password
# VM_STATE_RESTIC_PASSWORD, deduplicated and compressed, so nothing in it is plaintext and a backup uploads only what changed. Private, no
# PAR. Two identities read and write it: the admin key on agent-shell (the nightly backup and verify, through rclone's
# oracleobjectstorage backend) and the VM itself (cloud-init restores the volumes before any app starts), by its instance principal: the
# dynamic group and policy below, so the restore needs no secret beyond the restic password the vault already serves cloud-init.
#
# Versioning is ON: a deleted or overwritten object is recoverable for a day (the shortest lifecycle rule OCI takes), long enough to notice a
# bad prune from the nightly report. There is NO rule that expires current objects: the repository's old packs are live data (a 15-day expiry
# would destroy the snapshots it protects); retention is `restic forget --keep-last 14 --prune` of the backup verb. A prune deletes packs,
# and a delete on a versioned bucket keeps them as previous versions, still billed, so the cap counts them: the backup lists every object
# version (`oci os object list-object-versions`), refuses to start when the bucket plus what one run can add would pass
# vm_state_budget_bytes, and fails after its prune when the bucket or the live repository is over its budget.
#
# The tenancy's 20 GB cap is shared (always-free-quota.tf): the cache may hold 8 GB (budget.json storedBytes), this bucket at most
# vm_state_budget_bytes (every version of every object), which clis/blob-cache/data/budget.json repeats (vmStateBytes) and which is four
# times the sum of the maxBytes of the state volumes in vm-apps.json: twice for the live repository (vm_state_repository_bytes, budget.json
# vmStateRepositoryBytes) and twice for the previous versions a prune leaves for a day (a test keeps all of them equal). The storage guard lists this bucket and counts it, and never deletes from it
# (clis/blob-cache/src/storage_policy.rs Role::State).
# ===========================================================================

locals {
  # Sum of the caps: agent-workspace 1 GB + leetcode-backend-data 50 MB + vnc-auth-credentials 200 MB + orca-state 50 MB = 1.3 GB. The
  # workspace's kept part measured 0.25 GB on the live VM, so each cap is a multiple of what is there. 4 x for the bucket: 5.2 GB beside the
  # cache's 8 GB leaves 6.8 GB of the 20 GB tenancy cap for the evidence bucket.
  vm_state_budget_bytes     = 5200000000
  vm_state_repository_bytes = 2600000000
}

# The VM's own identity: every instance of the compartment (the recreated VM has a new id, so matching on one would race the restore).
resource "oci_identity_dynamic_group" "vm_state" {
  compartment_id = var.tenancy_ocid # dynamic groups are always tenancy-scoped
  name           = "${var.name_prefix}-vm-state-instances"
  description    = "The instances of the compartment: cloud-init restores the VM state from the backup repository with their instance principal."
  matching_rule  = "ALL {instance.compartment.id = '${var.compartment_ocid}'}"
}

resource "oci_identity_policy" "vm_state" {
  compartment_id = var.compartment_ocid
  name           = "${var.name_prefix}-vm-state-instances-policy"
  description    = "Lets the VM read and write the objects of the VM state bucket, and nothing else."
  statements = [
    "allow dynamic-group ${oci_identity_dynamic_group.vm_state.name} to manage objects in compartment id ${var.compartment_ocid} where target.bucket.name = '${var.name_prefix}-vm-state'",
  ]
}

resource "oci_objectstorage_bucket" "vm_state" {
  compartment_id = var.compartment_ocid
  namespace      = data.oci_objectstorage_namespace.ns.namespace
  name           = "${var.name_prefix}-vm-state"
  access_type    = "NoPublicAccess"
  storage_tier   = "Standard"
  versioning     = "Enabled"
}

# The lifecycle service's policy is build-cache.tf's `object_storage_lifecycle`; without it the apply 400s.
resource "oci_objectstorage_object_lifecycle_policy" "vm_state" {
  depends_on = [oci_identity_policy.object_storage_lifecycle]

  namespace = data.oci_objectstorage_namespace.ns.namespace
  bucket    = oci_objectstorage_bucket.vm_state.name

  rules {
    name        = "delete-previous-versions-after-1-day"
    action      = "DELETE"
    is_enabled  = true
    time_amount = 1
    time_unit   = "DAYS"
    target      = "previous-object-versions"
  }

  rules {
    name        = "abort-multipart-uploads-after-1-day"
    action      = "ABORT"
    is_enabled  = true
    time_amount = 1
    time_unit   = "DAYS"
    target      = "multipart-uploads"
  }
}

output "vm_state_bucket" {
  value       = oci_objectstorage_bucket.vm_state.name
  description = "The versioned bucket of the VM state restic repository (wf vm state). clis/deploy/src/vm_state/tools.rs BUCKET names the same string."
}
