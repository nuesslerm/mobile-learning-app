---
title: Giving Claude autonomous access to Oracle Cloud
date: 2026-08-30
category: infra
tags: [oracle-cloud, autonomy, free-tier, kill-switch]
problem_type: knowledge
module: infra/oracle-cloud
---

# Giving Claude autonomous access to Oracle Cloud — setup, free-tier safety, and a kill switch

**Status: Part 3 (the repo-side wiring) is done in code — registry entries,
`wf vm creds sync`, `terraform` pinned in `mise.toml`, and the
budget/alert-rule Terraform resources in `infra/oracle-cloud/budget.tf`. Parts
1 and 2 (the human-only OCI console steps — IAM user, API key, budget email)
are still outstanding, and nothing here has been applied to a live Oracle
Cloud account yet.** It explains what **you** (human-only, console/browser
steps) need to do, and what's now wired into this repo so a future Claude Code
session can run `terraform plan`/`apply`/`destroy` against
`infra/oracle-cloud/` without you hand-carrying credentials every time.

Companion reading: `infra/oracle-cloud/README.md` (the Terraform module this
document grants access *to*) and `docs/archive/solutions/infra/deployment-provisioning.md` (this
repo's general secrets/provisioning conventions, which everything below
follows).

---

## Read this first — two things that materially change the plan

### 1. The Always Free Ampere A1 allowance was cut in half, silently, on 2026-06-15

Oracle reduced the Always Free **Ampere A1 (ARM64)** compute allowance from
**4 OCPUs / 24 GB RAM** to **2 OCPUs / 12 GB RAM**, effective June 15, 2026 —
with no blog post, changelog entry, or account email. People found out when
their instances got reclaimed. ([Oracle's own free-tier
docs](https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier.htm) now
say: *"ensure that you have no more than 2 OCPUs and 12 GB of memory in total
across all the OCI Ampere A1 Compute instances in your tenancy"*; reporting on
the change: [InfoQ](https://www.infoq.com/news/2026/07/oracle-cloud-free-tier-limits/),
[TerminalBytes](https://terminalbytes.com/oracle-cloud-free-tier-changes-2026/).)

**RESOLVED.** This originally flagged that `variables.tf` still defaulted to
the pre-halving split — `light` (1 OCPU / 6 GB) + `heavy` (3 OCPU / 18 GB) =
4 OCPU / 24 GB, exactly double the new cap — and recommended either a single
combined 2 OCPU / 12 GB VM or two 1-OCPU/6 GB halves.

Both steps have since happened. The sizing was first cut to two 1-OCPU/6 GB
VMs, then consolidated to **one `VM.Standard.A1.Flex` VM at 2 OCPU / 12 GB**
(`oci_core_instance.main`, host label `main`), taking the whole current
allowance in a single instance. `light`/`heavy` no longer exist. Rationale
for dropping the split — and what it cost while it lasted — is in
`infra/oracle-cloud/README.md`'s "Why one VM".

### 2. "Always Free can't bill you" is a strong default, not an absolute guarantee

Oracle's own docs say *"your credit card will not be charged unless you
upgrade your account"* — genuinely the strongest default-safety design of the
big three clouds (AWS/GCP will happily bill an unupgraded free-tier account
the moment you exceed a limit; Oracle's API instead just refuses to create
the over-limit resource). **As long as the account is never upgraded to
Pay-As-You-Go, no Oracle Cloud action — including anything I run — can
generate a bill.** That's the primary kill switch, structural, not something
we configure.

But the June 2026 cut proves Oracle can and does change free-tier terms
unilaterally, without notice, and support agents have reportedly given
conflicting answers about whether the new 2/12 cap applies to Pay-As-You-Go
accounts too. So budgets, alerts, and a real teardown runbook (Part 2 below)
are not optional belt-and-suspenders — they're the actual safety net for the
scenario where Oracle's terms shift again, or a future decision to upgrade
for capacity reopens real billing risk.

**Recommendation: stay on the un-upgraded, true Always-Free account tier.**
Don't click "Upgrade account" in the console. If Ampere A1 capacity
("Out of host capacity" — already called out in `infra/oracle-cloud/README.md`
as common) becomes a recurring blocker, treat that as a deliberate,
explicit decision point to revisit — not something to route around silently
by upgrading.

---

## Part 1 — Oracle Cloud account setup (you do this; browser/console only)

I cannot do any of this part myself — it requires your Oracle account login,
phone/card verification, and org-level identity decisions only you should make.

### 1.1 Confirm the account is Always Free (not upgraded)

**Cloud Console → Account Management → Account Overview**, or the profile
menu. If it already shows "Pay As You Go," that's a separate conversation
before proceeding (the free-tier throttle-instead-of-bill protection no
longer applies at all, and everything in Part 2 becomes load-bearing rather
than defense-in-depth).

### 1.2 Create a dedicated IAM user + group for the agent (do not use your own account's credentials)

Reusing your personal/admin OCI login for anything automated is the OCI
equivalent of committing a personal AWS root key — don't. Create a
**separate IAM user** scoped to only what Terraform needs.

**Identity & Security → Domains → (your domain, usually `Default`) → Users → Create User**
- Name: something like `agent-terraform` (a service identity, not a person)
- No console password needed — this user only ever authenticates via API key

**Identity & Security → Domains → Groups → Create Group**
- Name: `agent-terraform-group`
- Add the `agent-terraform` user to it

**Identity & Security → Policies → Create Policy** (attach to the compartment
this infra lives in — root/tenancy compartment is fine for a personal
account, matching what `infra/oracle-cloud/README.md` already assumes):

```
Allow group agent-terraform-group to manage instance-family in compartment id <compartment-ocid>
Allow group agent-terraform-group to manage virtual-network-family in compartment id <compartment-ocid>
Allow group agent-terraform-group to manage volume-family in compartment id <compartment-ocid>
Allow group agent-terraform-group to read all-resources in compartment id <compartment-ocid>
Allow group agent-terraform-group to use budgets in compartment id <compartment-ocid>
```

This is deliberately **narrower than the account-wide admin policy** OCI's
quickstarts often suggest: it grants compute/network/storage lifecycle
management (create/update/destroy VMs, VCNs, volumes — everything
`infra/oracle-cloud/main.tf` actually touches) plus read-only visibility into
budgets, and nothing else. It cannot touch IAM, billing/payment methods,
other compartments, or any resource type Terraform doesn't already manage
here. If a future infra change needs a new resource type (e.g. Object
Storage, a Load Balancer), extend this policy explicitly rather than
widening it to `manage all-resources`.

### 1.3 Generate the API signing key for that user

While logged in as **yourself** (the API key is generated once, from the
target user's profile page, without requiring that user to log in):

**Identity & Security → Domains → Users → `agent-terraform` → API keys → Add API key → Generate API key pair**

- Download the private key. Do **not** save it anywhere in this repo's
  working tree — it goes straight into the vault (Part 3).
- The dialog shows a config preview with `tenancy`, `user`, `fingerprint`,
  and `region` — copy all four; you'll need them for the vault entries below.
- Note the **compartment OCID** too (root compartment = tenancy OCID for a
  personal account, same as `infra/oracle-cloud/README.md` already documents).

### 1.4 Set a hard IAM guardrail against non-Always-Free shapes (defense in depth)

Optional but cheap insurance against a future Terraform change (mine or
yours) accidentally specifying a billable shape. Add a compartment
**Quota** limiting compute to the Always-Free shape family:

**Governance → Quotas → Create Quota Policy**, scoped to the same
compartment:

```
zero standard-e-cores set to 0 in compartment id <compartment-ocid>
zero standard-b-cores set to 0 in compartment id <compartment-ocid>
```

(Adjust shape-family names to whatever's current in the Quotas console —
the goal: only `VM.Standard.A1.Flex`, the Ampere free shape, is
provisionable at all; everything else is quota-capped to zero. This makes
"agent accidentally picks a billable x86 shape" fail loudly at apply time
instead of quietly succeeding.)

---

## Part 2 — Cost monitoring and the kill switch (mostly console; some Terraform)

### 2.1 A budget with alert thresholds — now Terraform-managed, not console-only

**Done, in code:** `infra/oracle-cloud/budget.tf` provisions an
`oci_budget_budget` (default $1/month — deliberately paranoid given the
intent is "$0 forever", since any nonzero forecast at all means something
unexpected is being billed) plus `oci_budget_alert_rule` resources at
**50%, 80%, 100% actual spend, and 100% forecasted spend** — forecast catches
a runaway trend before it actually bills. Alert-rule emails go straight to
`recipients` (OCI budget alert rules email directly; no separate
Notifications topic needed — that machinery is only for the
automated-kill-switch pattern in Part 2.4).

Set `budget_alert_email` in `terraform.tfvars` to wire up the alert rules; the
budget itself is created even without an email, for console-visible cost
tracking. This makes the whole thing idempotent and re-appliable instead of a
one-off console click-through — no manual console steps left for this part.

### 2.2 Confirming the alert-rule email

OCI emails a confirmation link to each `budget_alert_email` recipient the
first time `terraform apply` creates the alert rules — an alert rule delivers
nothing until that link is clicked. Easy to miss; check for it right after
the first `apply` that sets `budget_alert_email`.

At this point you have **visibility**: any nonzero forecasted/actual cost
pages you within the hour. On an un-upgraded Always Free account this should
never fire; if it does, something's wrong with the account state itself
(e.g. an accidental upgrade), not just usage.

### 2.3 The actual kill switch — manual (always works, zero setup)

The one command that always stops the meter, regardless of anything else in
this document:

```bash
cd infra/oracle-cloud
terraform destroy
```

This tears down both VMs, the VCN, and everything Terraform manages. Keep
this as the documented, always-available emergency stop — a human (or me,
with your explicit go-ahead) running one command, not novel infrastructure
that itself needs maintaining.

### 2.4 The kill switch — automated (done, once the account upgraded to PAYG)

**Done, in code:** `infra/oracle-cloud/budget-guard.tf`, following the same
pattern as [Oracle's own writeup on enforced
budgets](https://blogs.oracle.com/cloud-infrastructure/enforced-budgets-on-oci-using-functions-and-quotas):

```
Budget alert fires (any ACTUAL threshold)
  → Events rule (matches com.oraclecloud.budgets.createtriggeredalert for
    this budget's OCID)
  → ONS Notifications topic (email — human visibility independent of the
    function's own outcome) + Oracle Function (budget-guard)
       - Function re-queries actual month-to-date spend itself (Budgets
         events don't reliably expose which specific threshold fired) and:
         >= budget_guard_destroy_threshold_pct (default 80%) → terminates
           both VMs outright
         >= budget_guard_stop_threshold_pct (default 50%) → stops both VMs
           (recoverable)
```

Gated behind the `function_image` variable, which only exists once you run
`infra/oracle-cloud/scripts/deploy-budget-guard-function.sh` — that script
needs a **running Docker daemon** (this repo's sandbox doesn't have one; OCI
Cloud Shell does, free, preconfigured). Until then `terraform apply` is a
no-op for this file.

`infra/oracle-cloud/scripts/retry-apply.sh` (the hourly capacity-retry cron)
has a matching interlock: it refuses to (re)create the VMs if actual spend is
already at/above the stop threshold, so it can't fight the kill switch by
recreating what was just torn down for overspending. See
`infra/oracle-cloud/README.md`'s "Automated kill switch" section for the full
setup + one flagged rough edge (an IAM policy statement I couldn't verify
against a live tenancy from here).

---

## Part 3 — Wiring the credentials into this repo (so I can use them autonomously)

This repo already has a registry-driven secrets system (`clis/secrets`,
`docs/archive/solutions/infra/deployment-provisioning.md`) — everything below follows that
convention rather than inventing a new one.

### 3.1 New registry entries — done

Added to `clis/secrets/src/data/registry.json` (mirroring the WHAT/WHERE/WHY-
IT-MATTERS/CONTROLS convention every existing entry uses):

| Key | Value | Notes |
| --- | --- | --- |
| `OCI_TENANCY_OCID` | tenancy OCID from step 1.3 | not secret-shaped, but keeping it alongside the others avoids splitting one logical credential set across two storage mechanisms |
| `OCI_USER_OCID` | the `agent-terraform` user's OCID | |
| `OCI_API_KEY_FINGERPRINT` | fingerprint from step 1.3 | |
| `OCI_API_PRIVATE_KEY` | the full PEM contents (multi-line) from step 1.3 | the sensitive one — scoped to compute/network/volume-manage only, per the narrow policy in 1.2, so a leak's blast radius is "attacker can resize/destroy these two VMs," not "attacker owns the tenancy" |
| `OCI_COMPARTMENT_OCID` | compartment OCID | |
| `OCI_REGION` | e.g. `eu-frankfurt-1` | |
| `OCI_API_PRIVATE_KEY_PASSPHRASE` | passphrase for `OCI_API_PRIVATE_KEY`, if encrypted | **optional** — a key generated via the OCI Console's "Add API key → Generate API key pair" button is unencrypted and needs none. Only set this if you generated the keypair yourself (e.g. `openssl`) with a passphrase and uploaded the public half to OCI |

New deploy group in `clis/secrets/src/secrets/groups.json` (`"oracle-terraform"`)
with the first six as `required` (there's no "partially configured" state that
makes sense for those — either Terraform can authenticate or it can't) and
`OCI_API_PRIVATE_KEY_PASSPHRASE` as `optional`.

**Still on you:** these are registry *entries* (WHAT/WHERE they're used) —
the registry doesn't hold values. Once you've done Part 1 and have the real
OCID/fingerprint/PEM values in hand, populate them with
`wf secret set OCI_TENANCY_OCID`, `wf secret set OCI_API_PRIVATE_KEY`
(reads from stdin if you omit the value — paste the PEM and Ctrl-D), etc.

### 3.2 A sync step that materializes them into what Terraform actually needs — done

`wf vm creds sync` (`clis/deploy/src/infra.rs`, `sync_creds`):

1. Pulls the six required values, plus the optional passphrase, via
   `clis/secrets/src/secrets/`'s `secret()`.
2. Writes the PEM to `~/.oci/oci_api_key.pem` (mode 600).
3. Writes/updates `infra/oracle-cloud/terraform.tfvars` (already gitignored,
   per `infra/oracle-cloud/.gitignore`) — but only its own
   marker-delimited block (`tenancy_ocid`, `user_ocid`,
   `api_key_fingerprint`, `api_private_key_path`,
   `api_private_key_passphrase`, `region`, `compartment_ocid`). Safe to
   re-run any time the vault values change: it never touches
   `ssh_public_key_path`, sizing overrides, `bws_read_access_token`,
   `budget_alert_email`, or anything else you've set by hand — those still
   come from `terraform.tfvars.example` per the README.

Fails loudly (exit 1, listing exactly which of the six *required* `OCI_*`
keys are missing) rather than writing a partial/broken tfvars — the
passphrase is optional, so its absence never blocks a sync (written as an
empty string, matching `versions.tf`'s `private_key_password` default).

### 3.3 Making `terraform`/`oci` CLI available wherever I run this — done (terraform)

`mise.toml` now pins `terraform = "latest"` (floor `>=1.5.0` matches
`infra/oracle-cloud/versions.tf`), installed via the same `SessionStart` hook
→ `.agents/setup.sh` → `step_mise` chain as `gh`. Verified in-session:
`mise install terraform`, then `terraform init` against `infra/oracle-cloud/`
successfully pulled the real `oracle/oci` provider (v8.24.0), and `terraform
validate` passed against the new `budget.tf` resources. The `oci` CLI itself
is still not provisioned (not strictly required — Terraform's `oci` provider
talks to the API directly) — add it to `mise.toml` later if you want it for
ad hoc checks.

---

## Part 4 — The operational runbook (once Parts 1–3 are done)

```bash
wf vm creds sync        # pulls the 6 secrets, writes ~/.oci/oci_api_key.pem + terraform.tfvars
cd infra/oracle-cloud
terraform init
terraform plan                    # always safe — read-only, shows what WOULD change
terraform apply                   # creates/updates real cloud resources — costs nothing on
                                   # Always Free, but is real infrastructure either way
terraform destroy                 # the manual kill switch (2.3) — tears everything down
```

### Confirmation policy

`terraform plan` is read-only and I'll run it freely once the above is
wired up. `terraform apply` and `terraform destroy` create/modify/destroy
real cloud infrastructure — exactly the "hard-to-reverse, affects shared
systems beyond this local environment" category my own operating principles
already flag for a check-first default. Unless you tell me otherwise, I'll
describe what a plan would do and ask before running `apply`/`destroy`,
every time — the same way I'd confirm before a force-push or a destructive
git operation. If you'd rather grant me standing authorization for routine
apply/destroy cycles on this specific module (not a blanket "always apply
anything anywhere"), the right place to record that durably is a note in
this repo's `AGENTS.md` or `CLAUDE.md` — say the word and I'll add it there
once we're at that point, so the authorization is explicit, scoped, and
visible to any future session reading those files, not just this
conversation.

---

## Ongoing safety checklist

- [ ] Account is Always Free, not Pay-As-You-Go (recheck after any Oracle
      account-related email — the June 2026 precedent means terms can shift
      without notice)
- [ ] `vm_ocpus ≤ 2` and `vm_memory_gbs ≤ 12` in
      `infra/oracle-cloud/terraform.tfvars` — one VM holds the whole pool, so
      these are the totals (re-verify against
      [Oracle's current free-tier
      page](https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier.htm)
      periodically — don't trust this document's numbers to stay current
      forever either)
- [ ] Budget + alert rules exist and the email/Slack subscription is
      confirmed (OCI sends a confirmation link on subscribe — easy to miss)
- [ ] The `agent-terraform` IAM policy is still scoped to
      instance/network/volume-family + budget-read, not `manage
      all-resources`
- [ ] You know where `terraform destroy` lives and that it's the
      always-available manual kill switch

## Sources

- [Oracle Cloud Infrastructure Free Tier — official docs](https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier.htm)
- [FAQ on Oracle's Cloud Free Tier](https://www.oracle.com/cloud/free/faq/)
- [Oracle Quietly Halves Free Tier Ampere A1 Compute Limits — InfoQ](https://www.infoq.com/news/2026/07/oracle-cloud-free-tier-limits/)
- [Oracle Cloud free tier 2026: 4 OCPU/24GB cut to 2 OCPU/12GB — TerminalBytes](https://terminalbytes.com/oracle-cloud-free-tier-changes-2026/)
- [Managing Budget Alert Rules — Oracle docs](https://docs.oracle.com/en-us/iaas/Content/Billing/Tasks/managingalertrules.htm)
- [Enforced budgets on OCI using functions and quotas — Oracle blog](https://blogs.oracle.com/cloud-infrastructure/enforced-budgets-on-oci-using-functions-and-quotas)
