# ===========================================================================
# Automated kill switch (docs/oracle-cloud-agent-provisioning.md Part 2.4) —
# an actual OCI-managed reactive pipeline, not a cron job polling for spend:
#
#   Budget alert fires (any threshold, ACTUAL type)
#     -> Events rule (matches com.oraclecloud.budgets.createtriggeredalert
#        for THIS budget's OCID)
#     -> ONS topic (email + PagerDuty, so a human/on-call sees "an alert
#        fired" independent of the function's own outcome)
#     -> OCI Function (budget-guard): re-queries actual month-to-date spend
#        itself (not the triggering event's payload — Budgets events don't
#        reliably expose which specific threshold fired) and decides:
#          >= destroy_threshold_pct of budget_amount_usd -> terminate both VMs
#          >= stop_threshold_pct of budget_amount_usd     -> stop both VMs
#          otherwise                                       -> no-op
#
# The ONS topic + email/PagerDuty subscriptions + Events rule below are
# gated only on `local.budget_alerting_enabled` (budget.tf) — they deliver
# notifications for every budget_alert_rule checkpoint regardless of whether
# the kill-switch Function itself has ever been deployed. The Function,
# its Application, and the IAM allowing it to actually stop/terminate
# compute are still gated behind `function_image`, which requires a real
# build+push to OCIR (scripts/deploy-budget-guard-function.sh — needs a
# machine with Docker AND an OCI Auth Token, see README section 7) — that
# part is NOT a no-op-safe default the way pure notification delivery is,
# since it grants "manage instance-family" to a function that can act on
# it autonomously.
# ===========================================================================

variable "function_image" {
  description = "Full OCIR image URI for the budget-guard function (e.g. eu-frankfurt-1.ocir.io/<namespace>/budget-guard-functions/budget-guard:0.0.1), produced by scripts/deploy-budget-guard-function.sh. Blank skips creating the Function/Application/IAM (the ONS topic + email/PagerDuty notifications above still work without it)."
  type        = string
  default     = ""
}

variable "budget_guard_stop_threshold_pct" {
  description = "Percent of budget_amount_usd at which the function stops (not terminates) both VMs — recoverable."
  type        = number
  default     = 50
}

variable "budget_guard_destroy_threshold_pct" {
  description = "Percent of budget_amount_usd at which the function terminates both VMs outright."
  type        = number
  default     = 100
}

locals {
  budget_guard_enabled = var.function_image != "" && local.budget_alerting_enabled
}

# ── Notifications topic + subscriptions — deliver on every budget_alert_rule
#    checkpoint, independent of whether the kill-switch Function exists ──

resource "oci_ons_notification_topic" "budget_guard" {
  count          = local.budget_alerting_enabled ? 1 : 0
  compartment_id = var.compartment_ocid
  name           = "${var.name_prefix}-budget-guard"
  description    = "Fires whenever a budget alert crosses a threshold. Delivered to email/PagerDuty here regardless of whether the stop/destroy Function is deployed; the function (if deployed) acts independently of this topic."
}

resource "oci_ons_subscription" "budget_guard_email" {
  count          = local.budget_alerting_enabled && var.budget_alert_email != "" ? 1 : 0
  compartment_id = var.compartment_ocid
  topic_id       = oci_ons_notification_topic.budget_guard[0].id
  protocol       = "EMAIL"
  endpoint       = var.budget_alert_email

  # OCI emails a confirmation link here too, same as every other ONS
  # subscription in this repo.
}

resource "oci_ons_subscription" "budget_guard_pagerduty" {
  count          = local.budget_alerting_enabled && var.pagerduty_integration_key != "" ? 1 : 0
  compartment_id = var.compartment_ocid
  topic_id       = oci_ons_notification_topic.budget_guard[0].id
  # Same CUSTOM_HTTPS pattern as security-alerts.tf's PagerDuty subscription
  # — see that file's own subscription resource for why plain "PAGERDUTY" or
  # "HTTPS" protocol types don't work for an EU PagerDuty service region.
  protocol = "CUSTOM_HTTPS"
  endpoint = "https://${var.pagerduty_events_domain}/integration/${var.pagerduty_integration_key}/enqueue"

  # Also pending until confirmed: OCI posts a confirmation payload to this
  # same URL, which a correctly-typed ("Custom Event Transformer") PagerDuty
  # integration confirms automatically — no click needed here.
}

# ── The function itself — the actual stop/destroy capability ───────────────

resource "oci_functions_application" "budget_guard" {
  count          = local.budget_guard_enabled ? 1 : 0
  compartment_id = var.compartment_ocid
  display_name   = "${var.name_prefix}-budget-guard"
  subnet_ids     = [oci_core_subnet.public.id]
}

resource "oci_functions_function" "budget_guard" {
  count              = local.budget_guard_enabled ? 1 : 0
  application_id     = oci_functions_application.budget_guard[0].id
  display_name       = "budget-guard"
  image              = var.function_image
  memory_in_mbs      = 128
  timeout_in_seconds = 30

  config = {
    TENANCY_OCID          = var.tenancy_ocid
    COMPARTMENT_OCID      = var.compartment_ocid
    BUDGET_AMOUNT_USD     = tostring(var.budget_amount_usd)
    STOP_THRESHOLD_PCT    = tostring(var.budget_guard_stop_threshold_pct)
    DESTROY_THRESHOLD_PCT = tostring(var.budget_guard_destroy_threshold_pct)
    # Still a comma-separated list (the function parses it as one) even though
    # there is a single VM now -- keeps adding a second host a one-token change.
    INSTANCE_IDS = oci_core_instance.main.id
  }
}

# ── IAM: the function's own resource principal can manage compute + read cost ──

resource "oci_identity_dynamic_group" "budget_guard_fn" {
  count          = local.budget_guard_enabled ? 1 : 0
  compartment_id = var.tenancy_ocid # dynamic groups are always tenancy-scoped
  name           = "${var.name_prefix}-budget-guard-fn"
  description    = "Matches the budget-guard function's resource principal."
  matching_rule  = "ALL {resource.type = 'fnfunc', resource.compartment.id = '${var.compartment_ocid}'}"
}

resource "oci_identity_policy" "budget_guard_fn" {
  count          = local.budget_guard_enabled ? 1 : 0
  compartment_id = var.tenancy_ocid
  name           = "${var.name_prefix}-budget-guard-fn-policy"
  description    = "Lets the budget-guard function stop/terminate the two VMs and read actual spend; lets the Events service invoke it."
  statements = [
    "allow dynamic-group ${oci_identity_dynamic_group.budget_guard_fn[0].name} to manage instance-family in compartment id ${var.compartment_ocid}",
    # Verify this exact resource-type against a live 403 on first deploy if
    # it doesn't match — Oracle's docs use both "usage-budgets" and
    # "usage-report" for cost-read access depending on source; IAM failures
    # here are safe (function just logs an error), not silently wrong.
    "allow dynamic-group ${oci_identity_dynamic_group.budget_guard_fn[0].name} to read usage-budgets in tenancy",
    "allow service cloudevents to use functions-family in compartment id ${var.compartment_ocid}",
  ]
}

# Publish rights for the ONS topic itself — kept separate from
# budget_guard_fn's policy above (which is function-only IAM) so the
# notification pipeline works even when function_image is blank.
resource "oci_identity_policy" "budget_guard_alerts" {
  count          = local.budget_alerting_enabled ? 1 : 0
  compartment_id = var.compartment_ocid
  name           = "${var.name_prefix}-budget-guard-alerts-policy"
  description    = "Lets the Events service publish budget-triggered alerts into the budget-guard ONS topic."
  statements = [
    "allow service cloudevents to use ons-topic in compartment id ${var.compartment_ocid}",
  ]
}

# ── Events rule: budget alert -> ONS topic (+ the function, if deployed) ──

resource "oci_events_rule" "budget_guard" {
  count          = local.budget_alerting_enabled ? 1 : 0
  compartment_id = var.compartment_ocid
  display_name   = "${var.name_prefix}-budget-guard-rule"
  description    = "Routes this budget's triggered alerts to the budget-guard function (if deployed) + a human-visible ONS topic (email/PagerDuty)."
  is_enabled     = true

  condition = jsonencode({
    eventType = ["com.oraclecloud.budgets.createtriggeredalert"]
    data = {
      additionalDetails = {
        budgetId = [oci_budget_budget.agent_cost_cap.id]
      }
    }
  })

  actions {
    actions {
      action_type = "ONS"
      is_enabled  = true
      topic_id    = oci_ons_notification_topic.budget_guard[0].id
    }

    dynamic "actions" {
      for_each = local.budget_guard_enabled ? [1] : []
      content {
        action_type = "FAAS"
        is_enabled  = true
        function_id = oci_functions_function.budget_guard[0].id
      }
    }
  }

  depends_on = [oci_identity_policy.budget_guard_alerts, oci_identity_policy.budget_guard_fn]
}
