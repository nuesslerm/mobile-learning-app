# ===========================================================================
# Verification artifacts (Always Free) -- publicly-readable, self-expiring
# storage for the evidence a verification run produces: capture-report HTML +
# PNGs, wf browser screenshots and recordings, explorer findings,
# runbook result JSON. GitHub issue #274.
#
# Same shape as the golden-screenshots bucket (object-storage.tf): anonymous
# GET on a known key works, so a link pasted into a GitHub epic opens on a
# phone with no login; the bucket cannot be listed anonymously. Unlike the
# golden bucket every object here is throwaway evidence, so a lifecycle rule
# deletes objects after 30 days and keeps the bucket inside the shared 20GB
# Object Storage quota (always-free-quota.tf) without any manual pruning.
#
# Writes go through one prefix-scoped PAR (`verification/*`, AnyObjectWrite)
# that `wf artifacts publish` PUTs against with plain curl -- no OCI CLI and
# no signing key on the publishing host (sandbox or agent-shell). The PAR is a
# credential: it lives in the vault as VERIFICATION_ARTIFACTS_PAR_URL, never
# in git, and expires 2027-09-01 (OCI PARs cannot be permanent).
# ===========================================================================

resource "oci_objectstorage_bucket" "verification_artifacts" {
  compartment_id = var.compartment_ocid
  namespace      = data.oci_objectstorage_namespace.ns.namespace
  name           = "${var.name_prefix}-verification-artifacts"
  access_type    = "ObjectReadWithoutList"
  storage_tier   = "Standard"
  versioning     = "Disabled"
}

resource "oci_objectstorage_object_lifecycle_policy" "verification_artifacts_expiry" {
  namespace = data.oci_objectstorage_namespace.ns.namespace
  bucket    = oci_objectstorage_bucket.verification_artifacts.name

  rules {
    name        = "delete-after-10-days"
    action      = "DELETE"
    is_enabled  = true
    time_amount = 10
    time_unit   = "DAYS"
    target      = "objects"
  }
}

resource "oci_objectstorage_preauthrequest" "verification_artifacts_write" {
  namespace    = data.oci_objectstorage_namespace.ns.namespace
  bucket       = oci_objectstorage_bucket.verification_artifacts.name
  name         = "verification-artifacts-write"
  access_type  = "AnyObjectWrite"
  object_name  = "verification/"
  time_expires = "2027-09-01T00:00:00Z"
}

output "verification_artifacts_public_url_prefix" {
  value       = "https://objectstorage.${var.region}.oraclecloud.com/n/${data.oci_objectstorage_namespace.ns.namespace}/b/${oci_objectstorage_bucket.verification_artifacts.name}/o/"
  description = "Prefix a URL-encoded object key (verification/<epic>/<branch>/<run>/index.html) onto this to get the publicly-readable evidence URL."
}

output "verification_artifacts_par_url" {
  value       = "https://objectstorage.${var.region}.oraclecloud.com${oci_objectstorage_preauthrequest.verification_artifacts_write.access_uri}"
  description = "Pre-authenticated PUT URL for keys under verification/. Treat as a credential -- store in the vault (VERIFICATION_ARTIFACTS_PAR_URL), never commit."
  sensitive   = true
}
