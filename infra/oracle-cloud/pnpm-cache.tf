# ===========================================================================
# Object Storage (Always Free) — the pnpm store cache bucket. No consumer is left (metro-runner, retired 22 Oct, was the
# only one); superseded by build-cache.tf, which now holds the lifecycle IAM policy this file's rule depends on.
#
# Private bucket (unlike golden_screenshots): a package tarball isn't
# sensitive, but there is no reason to make it anonymously fetchable, and a
# writable PAR is a real credential -- keep the whole surface private,
# reachable only via the scoped PAR below.
#
# One object per repo, always overwritten at the SAME key -- this is a
# deliberate design choice, not a missing feature: `pnpm install
# --prefer-offline` is content-addressed (a package is looked up by its own
# hash, not by which lockfile wrote it), so restoring last week's store
# before this week's install is always safe, never wrong -- new packages
# just get fetched fresh on top of it. A fixed key means every save is
# already the ONLY save (the PUT overwrites the prior blob), which is what
# "purge old cache after writing new one" reduces to when there is only ever
# one key: no separate list+delete purge step, no risk of the purge step
# itself failing and leaking storage. The key still encodes the repo
# identity (see pnpm_cache_key_prefix below) precisely so this bucket can
# host more than one repo/workspace's cache later without collisions.
# ===========================================================================

resource "oci_objectstorage_bucket" "pnpm_cache" {
  compartment_id = var.compartment_ocid
  namespace      = data.oci_objectstorage_namespace.ns.namespace
  name           = "${var.name_prefix}-pnpm-cache"
  access_type    = "NoPublicAccess"
  storage_tier   = "Standard"

  # Same rationale as golden_screenshots: regenerable content (worst case, a
  # cold pnpm install), well inside the Always-Free 20GB allowance (capped at
  # var.always_free_object_storage_cap_gb), versioning would only burn quota
  # on superseded stores with no recovery value.
  versioning = "Disabled"
}

# Repo-scoped key prefix every cache object lives under -- e.g.
# "expo-hello-world-app-2/pnpm-store.tar.gz". Deliberately just the repo
# name (not owner/repo, not branch): this bucket is scoped to one tenancy
# already, a repo name collision across two repos an agent works on in the
# same tenancy is the actual risk this guards against. (Per-branch scoping
# was rejected too: the retired consumer shared one store across every branch.)
locals {
  pnpm_cache_key_prefix = "expo-hello-world-app-2"
  pnpm_cache_key        = "${local.pnpm_cache_key_prefix}/pnpm-store.tar.gz"
}

# Long-dated (not permanent -- OCI PARs require an expiration) scoped
# credential: read+write the one object above, no bucket listing needed
# since there is only ever the one key. Narrower than a bucket-wide PAR
# would need to be, deliberately. Unused now, yet still a live write credential
# until this PAR or the bucket is dropped.
resource "oci_objectstorage_preauthrequest" "pnpm_cache_rw" {
  namespace   = data.oci_objectstorage_namespace.ns.namespace
  bucket      = oci_objectstorage_bucket.pnpm_cache.name
  name        = "pnpm-cache-rw"
  access_type = "ObjectReadWrite"
  object_name = local.pnpm_cache_key
  # ~2 years out. Rotation is a manual `terraform taint` +
  # apply away when this approaches expiry -- no auto-renew mechanism exists
  # for OCI PARs, so this WILL need a human/agent to notice and re-mint
  # eventually. Tracked as a known limitation, not solved here.
  time_expires = "2028-08-27T00:00:00Z"
}

output "pnpm_cache_par_url" {
  value       = "https://objectstorage.${var.region}.oraclecloud.com${oci_objectstorage_preauthrequest.pnpm_cache_rw.access_uri}"
  description = "Pre-authenticated PUT/GET URL for the one pnpm-store cache object this repo uses. Treat as a credential (write access) -- store in the vault (PNPM_CACHE_PAR_URL), never commit."
  sensitive   = true
}

# The retired task-output cache left objects under one prefix; the 7-day rule below expires them.
# Drop this rule and the prefix once the bucket holds none. The lifecycle IAM policy it needs is in build-cache.tf.
locals {
  stale_task_cache_prefix = "nx-cache/"
}

resource "oci_objectstorage_object_lifecycle_policy" "pnpm_cache" {
  depends_on = [oci_identity_policy.object_storage_lifecycle]

  namespace = data.oci_objectstorage_namespace.ns.namespace
  bucket    = oci_objectstorage_bucket.pnpm_cache.name

  rules {
    name        = "expire-stale-task-cache"
    action      = "DELETE"
    is_enabled  = true
    target      = "objects"
    time_amount = 7
    time_unit   = "DAYS"

    object_name_filter {
      inclusion_prefixes = [local.stale_task_cache_prefix]
    }
  }
}
