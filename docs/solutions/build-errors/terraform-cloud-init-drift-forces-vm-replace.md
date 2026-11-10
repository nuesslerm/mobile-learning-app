---
title: "terraform plan wanted to destroy+recreate both production VMs over a cloud-init edit that already ran"
date: 2026-07-30
category: build-errors
module: infra/oracle-cloud
problem_type: config_error
component: tooling
severity: critical
symptoms:
  - "terraform plan shows oci_core_instance.light/heavy \"must be replaced\" with no intentional instance-level change made"
  - "The forcing attribute is metadata (specifically the user_data cloud-init payload), shown as (sensitive value) in the diff"
  - "-target scoped to unrelated resources (e.g. budget/ONS resources) still includes the VM replace in both terraform plan -out=... and the resulting saved plan's JSON resource_changes"
root_cause: config_error
resolution_type: code_fix
related_components:
  - infra/oracle-cloud/main.tf
  - infra/oracle-cloud/cloud-init.sh.tftpl
tags: [terraform, oci, cloud-init, lifecycle, drift, targeting]
---

# terraform plan wanted to destroy+recreate both production VMs over a cloud-init edit that already ran

## Problem

An unrelated task (adding PagerDuty + more budget alert thresholds to `budget.tf`/`budget-guard.tf`) triggered a routine `terraform plan` before applying. The plan showed `oci_core_instance.light` and `oci_core_instance.heavy` — the two live production VMs — as `must be replaced`, forced by a diff on `metadata` (`user_data`, the base64-encoded cloud-init script, masked as `(sensitive value)` in the human-readable diff).

Applying this plan as-is would have destroyed and recreated both VMs: losing Tailscale device identity (a named volume on the boot disk), any other non-Compose-managed state, and potentially the reserved public IP — just to re-run a cloud-init script that had already run.

## What Didn't Work

**Assumed `-target` would protect the VMs from an incidental, unrelated diff.** Scoped `terraform plan -target=oci_budget_alert_rule.cost_alert -target=oci_ons_notification_topic.budget_guard ... -out=tfplan` to only the new/changed budget resources, expecting the VM replace to be excluded since nothing in the targeted set depends on the instances. It was not excluded — `terraform show -json tfplan` still listed `oci_core_instance.heavy -> ["delete","create"]` and the same for `light` in `resource_changes`, confirmed by parsing the JSON plan directly (not just eyeballing the human-readable summary, which is easy to misread when scanning past a wall of budget-alert-rule diffs). `-target` restricts what Terraform will *treat as the primary objects of the plan*, not what pre-existing forced-replacement drift gets included in the plan/apply — this diff was live and would have executed regardless of the target list.

The correct verification method, used before trusting any targeted plan against production infrastructure: `terraform show -json <planfile> | jq`-equivalent filtering to `resource_changes[].change.actions`, checking for anything other than `["no-op"]` on resources outside the intended target set — not just reading the colorized plan text, and not trusting `-target`'s scoping by assumption.

## Root Cause

`main.tf`'s `local.user_data` is `base64encode(templatefile("cloud-init.sh.tftpl", {...}))` — any change to `cloud-init.sh.tftpl`'s content changes the rendered output even with identical template variables. Three separate, already-merged commits after the VMs were created had legitimately fixed real bugs in that script (bws-install and vault-bootstrap credential seeding). Cloud-init only ever runs on an instance's first boot; none of those fixes could reach the already-running VMs without a real rebuild, so the fixes sat unapplied — and every `terraform plan` since has (correctly, by Terraform's own model) proposed a full instance replace to reconcile `user_data`, since `metadata` is one of the attributes that forces replacement on `oci_core_instance`.

This was initially misdiagnosed as a rotated secret (`bws_read_access_token`, one of the four template inputs) — a plausible alternate cause given the sensitive value and masked diff. Ruled out by comparing a SHA-256 hash of the token embedded in the live instance's state (`terraform.tfstate` stores `user_data` in plaintext regardless of the `sensitive` display flag) against the current `terraform.tfvars` value — identical hash, so the token was never the cause. `git log --since=<VM creation date> -- cloud-init.sh.tftpl` found the real cause directly: three commits touching the template itself.

## Solution

Added a `lifecycle { ignore_changes = [metadata] }` block to both `oci_core_instance.light` and `oci_core_instance.heavy` in `main.tf`. This is the standard Terraform pattern for exactly this situation: a first-boot-only provisioning payload shouldn't force a running instance to be rebuilt just because its own source template was edited afterward. A deliberate re-image now requires the explicit `terraform apply -replace=oci_core_instance.light` (or `.heavy`), rather than happening incidentally as a side effect of any future cloud-init edit.

Verified via the same `terraform show -json <planfile> | resource_changes[].change.actions` check: after adding the lifecycle block, a full (non-targeted) `terraform plan -out=tfplan` no longer includes `oci_core_instance.light`/`heavy` in its actionable changes at all, alongside the actually-intended budget/ONS resource changes.

## Why This Works

`ignore_changes = [metadata]` tells Terraform to stop comparing that attribute against the live resource when computing a plan, so drift in `local.user_data` (from any future cloud-init edit, template-variable change, or unrelated tfvars edit) can never again trigger a forced replace as a side effect. The instance's *other* attributes (shape, sizing, subnet, etc.) still plan normally — only the first-boot-only payload is exempted.

## Prevention

- **Any `oci_core_instance` (or other cloud provider's equivalent) whose `user_data`/`metadata` is templated from a script meant only for first-boot provisioning should carry `lifecycle { ignore_changes = [metadata] }` (or the attribute-specific equivalent) from the start** — not added reactively after the first scare. The failure mode is silent until someone runs a real `terraform plan`/`apply` long after the template was last edited.
- **Before applying any plan that touches shared/production infrastructure, inspect the plan's real JSON `resource_changes` actions, not just the human-readable summary** — especially when using `-target`, whose scoping guarantee does not extend to unrelated forced-replacement drift already present in the full diff.
- **When a `sensitive`-masked attribute is the forcing diff and a credential rotation seems like the obvious explanation, verify via a hash comparison against the plaintext value in the local state file** (`terraform.tfstate` stores everything in plaintext regardless of the `sensitive` flag, which only affects CLI/UI *display*) before accepting that story — `git log --since=<resource creation date> -- <template file>` is the other half of a fast, conclusive check, and is what actually found the real cause here.

## Related Issues

- `docs/solutions/build-errors/docker-image-arch-mismatch-arm64-vms.md` and its same-day update — a different Terraform/infra safety incident from the same general theme (an assumption about build/deploy state going unverified until a live check caught it), unrelated root cause.
