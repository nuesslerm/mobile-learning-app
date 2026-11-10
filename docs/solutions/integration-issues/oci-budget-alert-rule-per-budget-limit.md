---
title: "OCI Budgets silently caps a budget at exactly 10 alert rules — terraform apply fails partway through with 400-LimitExceeded"
date: 2026-07-31
category: integration-issues
module: "infra/oracle-cloud (Terraform budget alerting)"
problem_type: integration_issue
component: tooling
symptoms:
  - "terraform apply creates 9 of 10 new oci_budget_alert_rule resources then fails on the 10th with 400-LimitExceeded"
  - "Error text: \"The alert rule limit for this budget has been exceeded\" (Operation Name: CreateAlertRule)"
  - "terraform state list | grep oci_budget_alert_rule shows exactly 10 instances at the point of failure"
root_cause: config_error
resolution_type: config_change
severity: medium
tags: [terraform, oci, budgets, alert-rule, service-limit, oracle-cloud]
related_components: ["infra/oracle-cloud/budget-guard.tf"]
---

# OCI Budgets silently caps a budget at exactly 10 alert rules — `terraform apply` fails partway through with `400-LimitExceeded`

## Problem
Adding 10 new `ABSOLUTE`-threshold `oci_budget_alert_rule` resources to an OCI budget that already had a `FORECAST`/`PERCENTAGE` rule caused `terraform apply` to create 9 of the 10 and then fail on the 10th, because OCI Budgets enforces a hard, undocumented (in the Terraform resource schema) cap of exactly 10 alert rules per budget.

## Symptoms
- `terraform apply` destroys the old percentage-based rules successfully, creates 9 of the 10 new absolute-threshold rules, then fails on the 10th with:
  ```
  Error: 400-LimitExceeded, The alert rule limit for this budget has been exceeded.
  Suggestion: Request a service limit increase for this resource Budget Alert Rule
  Documentation: https://registry.terraform.io/providers/oracle/oci/latest/docs/resources/budget_alert_rule
  API Reference: https://docs.oracle.com/iaas/api/#/en/budgets/20190111/AlertRule/CreateAlertRule
  Request Target: POST https://usage.eu-frankfurt-1.oci.oraclecloud.com/20190111/budgets/<budget-ocid>/alertRules
  Provider version: 8.24.0
  Operation Name: CreateAlertRule

    with oci_budget_alert_rule.cost_alert["actual_17_50"],
    on budget.tf line 77, in resource "oci_budget_alert_rule" "cost_alert":
    77: resource "oci_budget_alert_rule" "cost_alert" {
  ```
- `terraform state list | grep oci_budget_alert_rule` shows exactly 10 rules at the point of failure — the 9 just-created absolute rules plus one pre-existing rule (`forecast_100`) whose `for_each` key was untouched by the diff and so was never destroyed.

## What Didn't Work
- Assuming this was a transient API error or an IAM/permissions problem and simply retrying the apply — it isn't transient and it isn't permissions-related. The error is deterministic: it reproduces on every attempt as soon as an 11th alert rule would exist on the same `budget_id`, regardless of how many retries are run.
- Reading the `oci_budget_alert_rule` Terraform resource schema for a limit — it documents only `threshold`, `threshold_type`, `type`, `budget_id`, `recipients`, `message`, etc., with no `max_items`/quota metadata anywhere. The Terraform provider docs page linked in the error and the generic OCI `AlertRule` API reference are the closest citable sources, and neither states "max 10 rules per budget" explicitly — the cap only became visible empirically, by counting rules in state after the failure.

## Solution
The pre-existing `forecast_100` alert rule was not part of the 10 newly-requested checkpoints and was left out of the refactored `budget_alert_rules` map, so it survived the `for_each` diff untouched — silently occupying the 10th slot before the 10 new rules had even finished being created. Since OCI's cap meant all 10 requested `ABSOLUTE` checkpoints could not coexist with any other rule on the same budget, `forecast_100` was dropped entirely from the config.

The current state of `infra/oracle-cloud/budget.tf` already reflects this fix — the `budget_alert_rules` local (`infra/oracle-cloud/budget.tf:55-62`) is built purely from the 10-entry `budget_actual_checkpoints` list (`infra/oracle-cloud/budget.tf:45`), with no separate forecast entry:

```hcl
# infra/oracle-cloud/budget.tf:45
budget_actual_checkpoints = [0.50, 1, 2, 5, 7.50, 10, 12.50, 15, 17.50, 20]

# infra/oracle-cloud/budget.tf:55-62
budget_alert_rules = {
  for amt in local.budget_actual_checkpoints :
  "actual_${replace(format("%.2f", amt), ".", "_")}" => {
    type           = "ACTUAL"
    threshold_type = "ABSOLUTE"
    threshold      = amt
  }
}
```

`oci_budget_alert_rule.cost_alert` (`infra/oracle-cloud/budget.tf:77-96`) then fans out over that map with a single `for_each`, so the resource count is driven entirely by the 10-entry local — there is no separate forecast resource block left anywhere in the file to reintroduce the 11th rule by accident.

Diagnosis and verification commands used, in order:
```bash
# 1. Scope the plan away from the unrelated pending VM replacement (documented separately)
terraform plan -target=oci_budget_budget.agent_cost_cap \
                -target=oci_budget_alert_rule.cost_alert \
                -target=oci_ons_notification_topic.budget_guard \
                -out=tfplan

# 2. Confirm only the intended resources would change before applying
terraform show -json tfplan | jq '.resource_changes[].change.actions'

# 3. Apply — this is the run that failed partway through
terraform apply tfplan

# 4. Count what actually exists in state after the partial failure
terraform state list | grep oci_budget_alert_rule
# -> 10 entries: 9 new "actual_*" rules + the untouched "forecast_100"

# 5. After dropping forecast_100 from the config, re-plan to confirm the
#    diff is now exactly: create actual_17_50, destroy forecast_100
terraform plan -target=oci_budget_alert_rule.cost_alert -out=tfplan
terraform apply tfplan

# 6. Verify final live state directly against the OCI API (not just Terraform state)
oci budgets budget alert-rule list --budget-id <budget-ocid> \
  --query "data[].{name:\"display-name\", type:type, thresholdType:\"threshold-type\", threshold:threshold}" \
  --output table
# -> exactly the 10 requested ABSOLUTE checkpoints (0.50 through 20.00), nothing else
```

## Why This Works
OCI Budgets enforces a hard per-budget cap of exactly 10 alert rules, enforced server-side by the Budgets API — not something the `oci_budget_alert_rule` Terraform resource schema exposes as a `max_items` constraint or validates client-side. Terraform's `for_each`-driven diff only destroys/creates resources whose map key changed; a rule whose key is untouched across a refactor (here, `forecast_100`) is left alone and silently keeps occupying a slot, so the true "how many rules will exist after this apply" count isn't visible from reading the diff for the *changed* resources alone — it requires accounting for every rule already in state on that `budget_id`, changed or not. Once the 10 new checkpoints plus the 1 surviving old rule summed to 11, the API rejected the 11th `CreateAlertRule` call outright; removing the untouched-but-still-present rule freed the slot the last new checkpoint needed.

## Prevention
- Before adding N new `oci_budget_alert_rule` entries to an existing budget, count what's already on that budget via `oci budgets budget alert-rule list --budget-id <id> --query "length(data)"` and confirm `N + (existing rules not being destroyed by this diff) <= 10` — don't infer the post-apply count from the Terraform plan's changed-resource list alone.
- If forecast/trend coverage needs to coexist with fine-grained `ACTUAL` checkpoints on the same budget, budget for only 9 `ACTUAL` checkpoints (leaving one slot for the `FORECAST` rule), not 10.
- The 10-rule cap is now documented directly in the Terraform config at `infra/oracle-cloud/budget.tf:47-54`, right above the `budget_alert_rules` local that consumes it, so a future edit adding more thresholds (or restoring forecast coverage) sees the constraint before hitting the same trial-and-error discovery again.

## Related Issues
- `docs/solutions/build-errors/terraform-cloud-init-drift-forces-vm-replace.md` — a separate, already-documented issue from the same session and the same `infra/oracle-cloud` Terraform stack (two production VM instances marked "must be replaced" due to stale cloud-init drift, deliberately excluded from this `terraform apply` via `-target` scoping). Different root cause, different files, different fix — related only by proximity in the same working session, not the same problem.
