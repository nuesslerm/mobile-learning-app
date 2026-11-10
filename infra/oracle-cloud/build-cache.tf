# ===========================================================================
# Object Storage (Always Free) -- the one build cache, shared by the sandbox, CI and the VM
# (clis/blob-cache, docs/plans/blob-cache/plan.md). Keys are <kind>/g<generation>/<name>.
#
# Private bucket: reads and writes go through the two bucket-wide PARs below, which wf resolves
# from the vault (BUILD_CACHE_READ_PAR_URL, BUILD_CACHE_WRITE_PAR_URL). Neither can list or delete;
# cleanup is the lifecycle policy here and `wf cache gc` with the admin key. Whole-bucket versioning
# stays off: a cache entry has no recovery value and would burn the shared 20GB quota.
# ===========================================================================

resource "oci_objectstorage_bucket" "build_cache" {
  compartment_id = var.compartment_ocid
  namespace      = data.oci_objectstorage_namespace.ns.namespace
  name           = "${var.name_prefix}-build-cache"
  access_type    = "NoPublicAccess"
  storage_tier   = "Standard"
  versioning     = "Disabled"
}

# A lifecycle policy only schedules deletions; this statement is what lets the Object Storage
# service run them. It also covers pnpm-cache.tf's rule (moved here: same address, so no state move).
# Confirmed live: without it the apply 400s with InsufficientServicePermissions.
resource "oci_identity_policy" "object_storage_lifecycle" {
  compartment_id = var.compartment_ocid
  name           = "${var.name_prefix}-object-storage-lifecycle-policy"
  description    = "Lets the Object Storage service execute the lifecycle (auto-delete) rules configured on buckets in this compartment -- required for oci_objectstorage_object_lifecycle_policy.pnpm_cache's expiry rule to actually run."
  # "objectstorage" alone 400s ("Service {objectstorage} does not exist") -- confirmed live.
  # The region-suffixed principal is the one OCI's lifecycle docs name.
  statements = [
    "allow service objectstorage-${var.region} to manage object-family in compartment id ${var.compartment_ocid}",
  ]
}

# The days must equal ttlDays in clis/blob-cache/data/budget.json (build_cache_tf test).
# Whole days from last modified; OCI has no last-access rule, so an entry hit daily still expires.
resource "oci_objectstorage_object_lifecycle_policy" "build_cache" {
  depends_on = [oci_identity_policy.object_storage_lifecycle]

  namespace = data.oci_objectstorage_namespace.ns.namespace
  bucket    = oci_objectstorage_bucket.build_cache.name

  # The API hands the rules back in this order (pnpm, nx, metro, sccache, probe, abort); any other order is a perpetual plan diff.
  rules {
    name        = "delete-pnpm-after-60-days"
    action      = "DELETE"
    is_enabled  = true
    time_amount = 60
    time_unit   = "DAYS"
    target      = "objects"

    object_name_filter {
      inclusion_prefixes = ["pnpm/"]
    }
  }

  rules {
    name        = "delete-nx-after-14-days"
    action      = "DELETE"
    is_enabled  = true
    time_amount = 14
    time_unit   = "DAYS"
    target      = "objects"

    object_name_filter {
      inclusion_prefixes = ["nx/"]
    }
  }

  rules {
    name        = "delete-metro-after-14-days"
    action      = "DELETE"
    is_enabled  = true
    time_amount = 14
    time_unit   = "DAYS"
    target      = "objects"

    object_name_filter {
      inclusion_prefixes = ["metro/"]
    }
  }

  rules {
    name        = "delete-sccache-after-7-days"
    action      = "DELETE"
    is_enabled  = true
    time_amount = 7
    time_unit   = "DAYS"
    target      = "objects"

    object_name_filter {
      inclusion_prefixes = ["sccache/"]
    }
  }

  rules {
    name        = "delete-probe-after-1-day"
    action      = "DELETE"
    is_enabled  = true
    time_amount = 1
    time_unit   = "DAYS"
    target      = "objects"

    object_name_filter {
      inclusion_prefixes = ["probe/"]
    }
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

# Bucket-wide (no object_name): the cache writes under several kind prefixes. Listing is denied
# explicitly. OCI PARs cannot be edited or made permanent: re-mint with `terraform taint` + apply
# before the expiry and re-set the secret.
resource "oci_objectstorage_preauthrequest" "build_cache_read" {
  namespace             = data.oci_objectstorage_namespace.ns.namespace
  bucket                = oci_objectstorage_bucket.build_cache.name
  name                  = "build-cache-read"
  access_type           = "AnyObjectRead"
  time_expires          = "2028-10-01T00:00:00Z"
}

resource "oci_objectstorage_preauthrequest" "build_cache_write" {
  namespace    = data.oci_objectstorage_namespace.ns.namespace
  bucket       = oci_objectstorage_bucket.build_cache.name
  name         = "build-cache-write"
  access_type  = "AnyObjectWrite"
  time_expires = "2028-10-01T00:00:00Z"
}

output "build_cache_read_par_url" {
  value       = "https://objectstorage.${var.region}.oraclecloud.com${oci_objectstorage_preauthrequest.build_cache_read.access_uri}"
  description = "Pre-authenticated GET/HEAD URL prefix for the build cache bucket; append the object key (nx/g0/<hash>). Treat as a credential -- store in the vault (BUILD_CACHE_READ_PAR_URL), never commit."
  sensitive   = true
}

output "build_cache_write_par_url" {
  value       = "https://objectstorage.${var.region}.oraclecloud.com${oci_objectstorage_preauthrequest.build_cache_write.access_uri}"
  description = "Pre-authenticated PUT URL prefix for the build cache bucket; append the object key. Treat as a credential -- store in the vault (BUILD_CACHE_WRITE_PAR_URL), never commit."
  sensitive   = true
}

# ===========================================================================
# Versioned shared libraries (docs/plans/versioned-libraries/plan.md, clis/lib): crates under libs/ of the build-cache bucket,
# read and written through prefix-restricted PARs (LIBS_READ_PAR_URL, LIBS_WRITE_PAR_URL). No lifecycle rule names libs/, so
# nothing there expires: `wf lib prune` is the only cleanup. The PARs below end in /o/libs/ and cannot list; a PAR cannot delete,
# so prune uses the admin key.
#
# The sparse index lives in its own bucket with public read (no listing): a Cargo lockfile records the registry's index URL, so
# that URL must not be a credential, and the index files are version numbers and checksums, nothing secret. The index's
# config.json `dl` points at the private crate PAR, which a lockfile never records.
# ===========================================================================

resource "oci_objectstorage_preauthrequest" "libs_read" {
  namespace             = data.oci_objectstorage_namespace.ns.namespace
  bucket                = oci_objectstorage_bucket.build_cache.name
  name                  = "libs-read"
  access_type           = "AnyObjectRead"
  object_name           = "libs/"
  bucket_listing_action = "Deny"
  time_expires          = "2028-10-01T00:00:00Z"

  # OCI does not return bucket_listing_action, so every plan saw it as an addition that forces a replacement: the recreate's
  # post-apply convergence check never reached "no changes" and rolled the new VM back. The value is set once, at creation.
  lifecycle {
    ignore_changes = [bucket_listing_action]
  }
}

resource "oci_objectstorage_preauthrequest" "libs_write" {
  namespace    = data.oci_objectstorage_namespace.ns.namespace
  bucket       = oci_objectstorage_bucket.build_cache.name
  name         = "libs-write"
  access_type  = "AnyObjectWrite"
  object_name  = "libs/"
  time_expires = "2028-10-01T00:00:00Z"
}

resource "oci_objectstorage_bucket" "libs_index" {
  compartment_id = var.compartment_ocid
  namespace      = data.oci_objectstorage_namespace.ns.namespace
  name           = "${var.name_prefix}-libs-index"
  access_type    = "ObjectReadWithoutList"
  storage_tier   = "Standard"
  versioning     = "Disabled"
}

resource "oci_objectstorage_preauthrequest" "libs_index_write" {
  namespace    = data.oci_objectstorage_namespace.ns.namespace
  bucket       = oci_objectstorage_bucket.libs_index.name
  name         = "libs-index-write"
  access_type  = "AnyObjectWrite"
  time_expires = "2028-10-01T00:00:00Z"
}

output "libs_read_par_url" {
  value       = "https://objectstorage.${var.region}.oraclecloud.com${oci_objectstorage_preauthrequest.libs_read.access_uri}"
  description = "Pre-authenticated GET/HEAD URL prefix for the published crates (libs/ of the build-cache bucket), ending /o/libs/. Treat as a credential -- store in the vault (LIBS_READ_PAR_URL), never commit."
  sensitive   = true
}

output "libs_write_par_url" {
  value       = "https://objectstorage.${var.region}.oraclecloud.com${oci_objectstorage_preauthrequest.libs_write.access_uri}"
  description = "Pre-authenticated PUT URL prefix for the published crates, ending /o/libs/. Treat as a credential -- store in the vault (LIBS_WRITE_PAR_URL), never commit."
  sensitive   = true
}

output "libs_index_public_url" {
  value       = "https://objectstorage.${var.region}.oraclecloud.com/n/${data.oci_objectstorage_namespace.ns.namespace}/b/${oci_objectstorage_bucket.libs_index.name}/o/"
  description = "Public (no listing) sparse-index URL of the libs registry: the `sparse+` source a Cargo lockfile records. Not a credential."
}

output "libs_index_write_par_url" {
  value       = "https://objectstorage.${var.region}.oraclecloud.com${oci_objectstorage_preauthrequest.libs_index_write.access_uri}"
  description = "Pre-authenticated PUT URL for the public index bucket. Treat as a credential -- store in the vault (LIBS_INDEX_WRITE_PAR_URL), never commit."
  sensitive   = true
}
