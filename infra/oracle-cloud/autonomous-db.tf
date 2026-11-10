# ===========================================================================
# Autonomous Database (Always Free) — durable storage for `wf check --health`
# snapshots, read back by the service-report frontend.
#
# Every Always-Free Autonomous Database ships with ORDS (Oracle REST Data
# Services) already running and internet-reachable by default — no separate
# API service needs to be built or deployed. A one-time setup step (the
# since-removed wf command `infra oracle db setup-health-reports`; SQL in git history) opened
# a SQL connection and runs `ORDS.ENABLE_SCHEMA` + `ORDS.ENABLE_OBJECT` on the
# `health_reports` table, after which:
#   GET  https://<hostname>/ords/<schema>/health_reports/  → JSON rows (paged)
#   POST https://<hostname>/ords/<schema>/health_reports/  → insert a row
# are both plain HTTPS, no OCI SDK, no DB driver in any long-running service.
#
# Deliberately NOT mTLS-only (is_mtls_connection_required = false): a wallet-
# based mTLS connection is what SQL clients / a Node driver would need, but
# ORDS's own HTTPS endpoint doesn't require the wallet at all — TLS alone
# (which every plain HTTPS client already speaks) is sufficient for calling
# ORDS. Turning off the mTLS *requirement* doesn't disable TLS; it only
# widens who's allowed to reach ORDS to also include the one-time setup
# script running from a sandboxed session with no wallet file.
# ===========================================================================

resource "oci_database_autonomous_database" "health_reports" {
  compartment_id = var.compartment_ocid
  # Max 14 chars, letters+digits only, must start with a letter.
  db_name       = "mlhealthdb"
  display_name  = "${var.name_prefix}-health-reports"
  admin_password = var.adb_admin_password

  # Always-Free shape: fixed at 1 OCPU / 20GB, auto-scaling off. Oracle
  # enforces these exact values for is_free_tier = true and rejects an
  # apply that asks for anything else -- data_storage_size_in_gb (confirmed
  # via `terraform validate` against the real provider schema; the more
  # guessable data_storage_size_in_gbs/_tbs names are NOT the real field)
  # is what expresses Always-Free's 20GB, since whole-TB granularity can't.
  #
  # cpu_core_count is deliberately UNSET, not "= 1": live against the real
  # API (2026-08-14), db_version = "23ai" free-tier databases report
  # cpu_core_count = 0 (compute is ECPU-managed, not legacy-OCPU-managed, for
  # this shape) and OCI rejects any attempt to change it away from 0 with
  # "This feature is not supported in an Always Free Autonomous AI Database."
  # Setting 1 here just makes every apply try and fail to "fix" a drift that
  # isn't one.
  is_free_tier            = true
  data_storage_size_in_gb = 20
  is_auto_scaling_enabled = false

  db_workload    = "OLTP"
  db_version     = "23ai"
  license_model  = "LICENSE_INCLUDED"

  # Left at OCI's default (true). Two things were tried and ruled out live
  # against the real API before landing here (2026-08-14):
  #   - is_mtls_connection_required = false alone: rejected --
  #     "One-way TLS connections require a private endpoint, a VCN with an
  #     ACL, or a public IP with an ACL."
  #   - + is_access_control_enabled/whitelisted_ips to satisfy that: rejected
  #     too -- "Configure access control for Autonomous AI Database is not
  #     supported." (db_version = "23ai" puts this shape in OCI's "Autonomous
  #     AI Database" category, which doesn't expose ACL config at all.)
  # Neither path is available without a VCN, which this deliberately-simple
  # single-DB setup doesn't have. This is fine: ORDS's own REST endpoint is a
  # separate HTTPS service that was never gated by this setting in the first
  # place (see file header) -- only a *direct SQL*Net connection* (the
  # one-time schema setup, now removed) needs mTLS's wallet.

  # A rename or password change on an existing Always-Free ADB is an
  # in-place update, not a replace -- but a workload/db_name change is not
  # supported at all by the OCI API and would force a destroy+recreate,
  # losing every stored health-report row. There is exactly one Always-Free
  # ADB slot commonly used this way per tenancy region; treat this resource
  # like the main VM (main.tf) rather than something casually re-shaped.
  lifecycle {
    ignore_changes = [admin_password]
  }
}
