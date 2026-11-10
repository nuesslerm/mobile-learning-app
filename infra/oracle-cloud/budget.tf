# ===========================================================================
# Cost monitoring — Terraform-managed equivalent of
# docs/oracle-cloud-agent-provisioning.md Part 2.1, so the budget + alert
# rules are idempotent and re-appliable instead of a one-off console
# click-through.
#
# The budget itself is created unconditionally (it's free and gives
# console-visible cost tracking even with no alerting wired up). The alert
# rules only exist when `budget_alert_email` or `pagerduty_integration_key`
# is set — an alert rule needs somewhere to send to. The automated-kill-switch
# pipeline (Events rule -> ONS topic -> Function that stops/terminates the
# VMs) is built — see budget-guard.tf — gated behind `function_image` being
# set; the alert *notifications* themselves (email + PagerDuty) no longer
# depend on that function existing — see budget-guard.tf's own header.
# ===========================================================================
variable "budget_alert_email" {
  description = "Comma-separated email address(es) to receive budget alert notifications. Leave blank to skip creating alert rules (the budget itself is still created for console visibility)."
  type        = string
  default     = ""
}

variable "budget_amount_usd" {
  description = "Monthly budget cap in USD. Also the base budget-guard.tf computes its stop/destroy thresholds against (see budget_guard_stop_threshold_pct / budget_guard_destroy_threshold_pct)."
  type        = number
  default     = 20
}

locals {
  # PagerDuty alone (no email) is enough to want the alert rules + the
  # budget-guard ONS topic/Events rule to exist — matches security-alerts.tf's
  # own gating pattern (var.security_alert_email != "" || var.pagerduty_integration_key != "").
  # nonsensitive(): this boolean only says whether the key is set, never the
  # key's value itself, but it feeds a for_each (not just a count) below,
  # where Terraform additionally refuses any sensitive-derived value outright.
  budget_alerting_enabled = var.budget_alert_email != "" || nonsensitive(var.pagerduty_integration_key) != ""

  # ABSOLUTE, not PERCENTAGE: these are literal currency checkpoints across
  # the full budget range, requested explicitly (0.50/1/2/5/7.50/10/12.50/
  # 15/17.50/20). Pinning them as absolute amounts means they keep meaning
  # the same real spend level even if budget_amount_usd is later changed —
  # a PERCENTAGE threshold would silently rescale instead. threshold_type
  # ABSOLUTE supports up to 2 decimal digits (confirmed against the oci CLI's
  # own generated help for `budgets budget alert-rule create`), which covers
  # the .50 amounts here.
  budget_actual_checkpoints = [0.50, 1, 2, 5, 7.50, 10, 12.50, 15, 17.50, 20]

  # NOTE: OCI caps a budget at exactly 10 alert rules (confirmed live —
  # "400-LimitExceeded, The alert rule limit for this budget has been
  # exceeded" on the 11th create). The 10 ACTUAL checkpoints below use the
  # entire budget; there's no room left for a separate FORECAST rule (the
  # previous 50/80/100 ACTUAL + 100 FORECAST setup fit in 4 — this one is a
  # deliberate tradeoff for finer-grained ACTUAL checkpoints instead of
  # forecast/trend coverage). If forecast coverage is wanted back, one of
  # these 10 checkpoints has to go.
  budget_alert_rules = {
    for amt in local.budget_actual_checkpoints :
    "actual_${replace(format("%.2f", amt), ".", "_")}" => {
      type           = "ACTUAL"
      threshold_type = "ABSOLUTE"
      threshold      = amt
    }
  }
}

# Budgets always live in the TENANCY compartment in OCI, scoped down to the
# actual resources via targets (target_compartment_id is deprecated).
resource "oci_budget_budget" "agent_cost_cap" {
  compartment_id                        = var.tenancy_ocid
  targets                               = [var.compartment_ocid]
  display_name                          = "${var.name_prefix}-agent-cost-cap"
  description                           = "Paranoid cap on an intended-$0 Always-Free setup - see docs/oracle-cloud-agent-provisioning.md."
  amount                                = var.budget_amount_usd
  reset_period                          = "MONTHLY"
  budget_processing_period_start_offset = 1
}

resource "oci_budget_alert_rule" "cost_alert" {
  for_each = local.budget_alerting_enabled ? local.budget_alert_rules : {}

  budget_id      = oci_budget_budget.agent_cost_cap.id
  display_name   = "${var.name_prefix}-${each.key}"
  type           = each.value.type
  threshold      = each.value.threshold
  threshold_type = each.value.threshold_type
  message        = each.value.threshold_type == "ABSOLUTE" ? "Budget '${oci_budget_budget.agent_cost_cap.display_name}' hit ${each.value.threshold} ${lower(each.value.type)} spend — an Always-Free setup should never see a nonzero actual; check for an accidental account upgrade or an out-of-policy resource." : "Budget '${oci_budget_budget.agent_cost_cap.display_name}' hit ${each.value.threshold}% ${lower(each.value.type)} — an Always-Free setup should never see a nonzero forecast/actual; check for an accidental account upgrade or an out-of-policy resource."
  # OCI's alert-rule `recipients` field only accepts email addresses — there
  # is no native PagerDuty/webhook recipient on the rule itself. PagerDuty
  # delivery for these same alerts goes through budget-guard.tf's ONS topic
  # instead (every rule here fires the createtriggeredalert event that
  # topic's Events rule listens for, regardless of `recipients`).
  recipients = var.budget_alert_email

  # OCI emails a confirmation link to each recipient on creation — the rule
  # delivers nothing until that link is clicked. Easy to miss; see the
  # ongoing safety checklist in docs/oracle-cloud-agent-provisioning.md.
}
