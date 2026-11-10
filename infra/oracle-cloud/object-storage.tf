# ===========================================================================
# Object Storage (Always Free) — durable, publicly-readable storage for
# golden-screenshot regression test images (clis/multi-browser/tests/golden).
#
# A golden run's baseline/actual/diff PNGs are useful across MANY runs (a
# history view needs to show what a screen looked like weeks ago, not just
# the latest capture) but are far too large to embed in the JSON already
# published to the health_reports Autonomous DB (see that table's own doc
# comment in autonomous-db.tf and testCaseResultSchema's doc comment in
# frontend/mla-tamagui/features/service-report/src/contracts/healthReport.ts) -- a few hundred KB
# per screenshot, times ~10 screenshots, times every historical run, would
# make every future health-report row carry that weight forever. Object
# Storage is the right tier for this: cheap, durable blob storage that a
# health-report row can point at by URL instead of embedding.
#
# `access_type = "ObjectReadWithoutList"`: anonymous GET on a known object
# name/URL works (so the dashboard's <Image> / <img> tags can load a
# screenshot URL directly, no signed request needed), but the bucket cannot
# be anonymously LISTED -- a stranger can't enumerate every screenshot ever
# taken just by guessing the bucket exists, only fetch one whose exact
# (guessable-only-if-you-already-know-the-run-id) key they already have.
# None of this content is sensitive (UI screenshots of a demo app), so public
# object read is an acceptable, deliberate tradeoff for a zero-backend image
# host -- there is no server in this repo whose job would otherwise be
# "authenticate a screenshot fetch".
# ===========================================================================

data "oci_objectstorage_namespace" "ns" {
  compartment_id = var.compartment_ocid
}

resource "oci_objectstorage_bucket" "golden_screenshots" {
  compartment_id = var.compartment_ocid
  namespace      = data.oci_objectstorage_namespace.ns.namespace
  name           = "${var.name_prefix}-golden-screenshots"
  access_type    = "ObjectReadWithoutList"
  storage_tier   = "Standard"

  # Screenshots are regenerable (re-run the golden suite) and this bucket is
  # capped well inside the Always-Free 20GB allowance (see
  # always_free_object_storage_cap_gb in always-free-quota.tf) -- versioning
  # would only burn quota on superseded baselines with no real recovery
  # value, unlike the tfstate bucket's own versioning (external, not managed
  # by this Terraform, and genuinely irreplaceable).
  versioning = "Disabled"
}

output "golden_screenshots_bucket_url_prefix" {
  value       = "https://objectstorage.${var.region}.oraclecloud.com/n/${data.oci_objectstorage_namespace.ns.namespace}/b/${oci_objectstorage_bucket.golden_screenshots.name}/o/"
  description = "Prefix a URL-encoded object name onto this to get a publicly-readable screenshot URL."
}
