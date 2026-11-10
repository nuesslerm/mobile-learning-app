# ===========================================================================
# Security alerting — real-time Audit-event alerts for exactly the actions a
# breached API key (or an agent bug) could take that our other guardrails
# don't already cover:
#
#   - always-free-quota.tf blocks non-free compute/storage at REQUEST time,
#     but doesn't tell anyone an attempt happened. Worse: Limits/Quotas
#     itself doesn't emit Events at all (verified against OCI's own
#     "Services that Produce Events" reference — it's not in the list), so
#     a stolen key widening our quota to lift the cap would go silent here.
#     Mitigated by scripts/check-quota-drift.sh — periodic drift detection
#     (compares the live quota against what Terraform declares, alerts
#     directly to this file's own ONS topic if they differ) instead of a
#     real-time Event, run on a schedule since none exists for this
#     resource type. See README section 8.
#   - budget-guard.tf reacts to SPEND, which lags reality by design (usage
#     data isn't instant).
#   - Nothing else watches for a NEW CREDENTIAL, a NEW USER/POLICY, or a
#     NETWORK SECURITY CHANGE — the classic signs of a compromised key
#     being used to persist access or open a hole, independent of whether
#     anything ever gets billed.
#
# Pattern: Audit -> Events rule (matches a fixed allowlist of high-risk
# eventTypes) -> ONS topic -> email + (optional) PagerDuty. Everything on
# the allowlist is something ONLY retry-apply.sh / `terraform destroy` /
# the console should ever legitimately do, so a match here is inherently
# rare and worth a human looking at — no dedup/threshold logic needed the
# way budget-guard.tf needs one for noisy spend data.
#
# Known gap #1: a quota update/delete (`oci limits quota update|delete`)
# doesn't fire an Event, so it slips past this rule specifically.
#
# Known gap #2 (CONFIRMED LIVE, bigger than #1): identity-plane events
# (identityControlPlane.* -- new API keys, auth tokens, users, groups,
# policies) do NOT reach this rule in this tenancy, even though they're
# logged in Audit with the exact eventType strings this rule matches.
# Verified with a controlled A/B test: creating+deleting an IAM group and
# an auth token produced zero deliveries (checked via the ONS topic's own
# PublishedMessagesCount metric AND the Events service's own "Events
# Emitted" chart -- both zero), while creating an NSG (a core Networking
# event, not identity-plane) delivered correctly and paged as expected,
# with the rule, topic, subscription, and PagerDuty transform all
# completely unchanged between the two tests. This tenancy uses Identity
# Domains (confirmed via IDCS/SCIM-shaped fields in the Audit payload
# itself, e.g. `idcsCreatedBy`, `urn:ietf:params:scim:schemas:oracle:idcs:
# authToken`) -- Oracle's own Events reference lists "IAM" as an
# Events-producing service, but that appears to predate Identity Domains;
# live behavior here says identity-plane events specifically aren't wired
# into the classic Events pipeline for a Identity-Domains tenancy, even
# though core-service (compute/network) events are.
#
# Net effect: THIS FILE DOES NOT ALERT ON NEW CREDENTIALS/USERS/POLICIES
# RIGHT NOW, despite being configured to. The IAM eventTypes are left in
# the condition below in case Oracle fixes this (harmless if so -- they
# just never match today), but don't rely on this file for that category.
# OCI Cloud Guard's IAM Activity Detector (free, tenancy-wide, one-time
# console toggle -- not Terraformed here) reads Audit directly rather
# than going through the Events pipeline, so it isn't necessarily subject
# to the same gap -- it's the one to actually turn on for credential/IAM
# monitoring given this finding, not just "defense in depth" as originally
# framed.
# ===========================================================================

variable "security_alert_email" {
  description = "Email to receive security alerts (new API keys/users/policies, security list/NSG changes, compute launch/terminate). Leave blank to skip email (PagerDuty can be used alone)."
  type        = string
  default     = ""
}

variable "pagerduty_integration_key" {
  description = "PagerDuty integration key from a service's 'Custom Event Transformer' integration — NOT the 'Oracle Cloud Infrastructure Monitoring' integration type, which silently ignores anything that isn't an alarm-format message (Events/ONS payloads aren't). Leave blank to skip PagerDuty."
  type        = string
  default     = ""
  sensitive   = true
}

variable "pagerduty_events_domain" {
  description = "PagerDuty Events API host for your account's service region — 'events.pagerduty.com' (US, default) or 'events.eu.pagerduty.com' (EU). Check which region your account is on: an EU account's own PagerDuty URL is <subdomain>.eu.pagerduty.com. Wrong region doesn't error, it just silently never delivers."
  type        = string
  default     = "events.pagerduty.com"
}

locals {
  security_alerts_enabled = var.security_alert_email != "" || var.pagerduty_integration_key != ""

  security_alert_event_types = [
    # --- IAM: new/changed credentials, users, groups, policies. Verified
    # against OCI's Events reference — identityControlPlane is one of the
    # few services whose eventType verb is PascalCase, not lowercase. ---
    "com.oraclecloud.identityControlPlane.CreateUser",
    "com.oraclecloud.identityControlPlane.UpdateUser",
    "com.oraclecloud.identityControlPlane.DeleteUser",
    "com.oraclecloud.identityControlPlane.UpdateUserCapabilities",
    "com.oraclecloud.identityControlPlane.CreateGroup",
    "com.oraclecloud.identityControlPlane.UpdateGroup",
    "com.oraclecloud.identityControlPlane.DeleteGroup",
    "com.oraclecloud.identityControlPlane.AddUserToGroup",
    "com.oraclecloud.identityControlPlane.RemoveUserFromGroup",
    "com.oraclecloud.identityControlPlane.CreatePolicy",
    "com.oraclecloud.identityControlPlane.UpdatePolicy",
    "com.oraclecloud.identityControlPlane.DeletePolicy",
    "com.oraclecloud.identityControlPlane.UploadApiKey",
    "com.oraclecloud.identityControlPlane.DeleteApiKey",
    "com.oraclecloud.identityControlPlane.CreateAuthToken",
    "com.oraclecloud.identityControlPlane.UpdateAuthToken",
    "com.oraclecloud.identityControlPlane.DeleteAuthToken",
    "com.oraclecloud.identityControlPlane.CreateCustomerSecretKey",
    "com.oraclecloud.identityControlPlane.UpdateCustomerSecretKey",
    "com.oraclecloud.identityControlPlane.DeleteCustomerSecretKey",
    "com.oraclecloud.identityControlPlane.CreateSmtpCredential",
    "com.oraclecloud.identityControlPlane.CreateSwiftPassword",
    "com.oraclecloud.identityControlPlane.CreateOrResetPassword",

    # --- Networking: security list / NSG changes. main.tf only ever
    # manages the default security list once at apply time, so any change
    # outside a deliberate `terraform apply` is a signal. ---
    "com.oraclecloud.virtualnetwork.createsecuritylist",
    "com.oraclecloud.virtualnetwork.updatesecuritylist",
    "com.oraclecloud.virtualnetwork.deletesecuritylist",
    "com.oraclecloud.virtualnetwork.changesecuritylistcompartment",
    "com.oraclecloud.virtualnetwork.createnetworksecuritygroup",
    # Pattern-inferred from the entry above, not individually confirmed
    # against a live event — if this exact string is wrong, only NSG rule
    # *updates* (not the create above) go unmatched.
    "com.oraclecloud.virtualnetwork.updatenetworksecuritygroupsecurityrules",

    # --- Compute: instance lifecycle. Only retry-apply.sh, `terraform
    # destroy`, or budget-guard's function should ever trigger these.
    # KNOWN GAP, verified live via scripts/test-quota-block.sh: a launch
    # always-free-quota.tf rejects does NOT fire either event — OCI only
    # emits these for a request it actually starts acting on, not one it
    # synchronously rejects before any work begins. So a blocked
    # non-free-shape attempt is invisible here; these two only cover a
    # launch that actually proceeds (e.g. quota misconfigured/widened, or
    # the free-tier shapes themselves). always-free-quota.tf's own
    # rejection (visible in the API response, not an alert) is still what
    # actually stops it — this category just can't tell you it happened. ---
    "com.oraclecloud.computeapi.launchinstance.begin",
    "com.oraclecloud.computeapi.launchinstance.end",
    "com.oraclecloud.computeapi.terminateinstance.begin",
    "com.oraclecloud.computeapi.terminateinstance.end",
  ]
}

resource "oci_ons_notification_topic" "security_alerts" {
  count          = local.security_alerts_enabled ? 1 : 0
  compartment_id = var.compartment_ocid
  name           = "${var.name_prefix}-security-alerts"
  description    = "New credentials/users/policies, security list/NSG changes, or compute launch/terminate — anything a breached key or a bug could do outside a deliberate terraform apply."
}

resource "oci_ons_subscription" "security_alerts_email" {
  count          = local.security_alerts_enabled && var.security_alert_email != "" ? 1 : 0
  compartment_id = var.compartment_ocid
  topic_id       = oci_ons_notification_topic.security_alerts[0].id
  protocol       = "EMAIL"
  endpoint       = var.security_alert_email

  # OCI emails a confirmation link here — delivers nothing until it's
  # clicked, same as every other ONS subscription in this repo.
}

resource "oci_ons_subscription" "security_alerts_pagerduty" {
  count          = local.security_alerts_enabled && var.pagerduty_integration_key != "" ? 1 : 0
  compartment_id = var.compartment_ocid
  topic_id       = oci_ons_notification_topic.security_alerts[0].id
  # NOT "PAGERDUTY", and NOT plain "HTTPS" either: OCI's own
  # CreateSubscription API hardcodes URL validation on both of those
  # protocol types to the literal prefix
  # "https://events.pagerduty.com/integration/" — verified live (both 400
  # identically), it rejects anything else, including the EU host. That
  # makes both unusable for a PagerDuty account on the EU service region.
  # "CUSTOM_HTTPS" is the one OCI protocol type that skips this validation
  # (also verified live — the subscription reached ACTIVE) while still
  # delivering a plain HTTPS POST, which is all the PagerDuty side needs:
  # it's a "Custom Event Transformer" integration specifically because it
  # parses whatever raw payload arrives, not a format tied to OCI's
  # PAGERDUTY-labeled wire type.
  protocol = "CUSTOM_HTTPS"
  endpoint = "https://${var.pagerduty_events_domain}/integration/${var.pagerduty_integration_key}/enqueue"

  # Also pending until confirmed: OCI posts a confirmation payload to this
  # same URL, which a correctly-typed ("Custom Event Transformer")
  # PagerDuty integration confirms automatically — no click needed here.
}

# Lets the Events service publish into this specific topic. Overlaps in
# substance with budget-guard.tf's own "cloudevents to use ons-topic"
# statement, but each policy resource is self-contained — this file works
# standalone whether or not budget-guard.tf's function_image is set, and
# OCI tolerates the redundant grant across two policies without conflict.
resource "oci_identity_policy" "security_alerts" {
  count          = local.security_alerts_enabled ? 1 : 0
  compartment_id = var.compartment_ocid
  name           = "${var.name_prefix}-security-alerts-policy"
  description    = "Lets the Events service publish matched security events into the security-alerts ONS topic."
  statements = [
    "allow service cloudevents to use ons-topic in compartment id ${var.compartment_ocid}",
  ]
}

resource "oci_events_rule" "security_alerts" {
  count = local.security_alerts_enabled ? 1 : 0
  # var.tenancy_ocid, NOT var.compartment_ocid: an Events rule only matches
  # events in the compartment it's created in PLUS descendants, never
  # ancestors (verified live: zero oci_notification metrics ever recorded
  # for this topic, and the test CreateAuthToken/DeleteAuthToken events
  # both showed compartment-id = the tenancy). Identity-plane events
  # (users, groups, policies, API keys, auth tokens) are always
  # tenancy-scoped — a rule anywhere below the tenancy can never see them.
  # Scoping the rule at the tenancy still covers compute/network events in
  # var.compartment_ocid too, since that's a descendant of the tenancy.
  compartment_id = var.tenancy_ocid
  display_name   = "${var.name_prefix}-security-alerts-rule"
  description    = "Routes high-risk IAM/network/compute audit events to the security-alerts topic in real time."
  is_enabled     = true

  condition = jsonencode({
    eventType = local.security_alert_event_types
  })

  actions {
    actions {
      action_type = "ONS"
      is_enabled  = true
      topic_id    = oci_ons_notification_topic.security_alerts[0].id
    }
  }

  depends_on = [oci_identity_policy.security_alerts]
}
