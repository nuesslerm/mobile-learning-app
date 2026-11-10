# Oracle Cloud infra for the backend services (Terraform)

Provisions everything the four backend deploy workflows need on Oracle Cloud's
**Always-Free Ampere A1 (ARM64)** pool:

- 1 VCN + public subnet + internet gateway + route table
- A security list that allows **inbound SSH (22) only** — all app traffic rides
  Tailscale (outbound-initiated), so no public app ports are opened
- **1 VM** (`VM.Standard.A1.Flex`, ARM64), display name `<prefix>-main`,
  host label `main`, running **every** app: `leetcode-backend`,
  `remote-chat-agent`, `leetcode-auth-cookie-capture`,
  plus the `vm-gateway` → `vm-ops-api` control-plane pair
  - Takes the CURRENT free cap **whole**: 2 OCPU / 12 GB + a 150 GB boot volume
    (Oracle halved the A1 pool from 4/24 on 2026-06-15); stays $0 on the free tier
  - **Was two VMs** (`light` 1/6 + `heavy` 1/6). The names described a real
    difference only before the halving (`heavy` was 3 OCPU / 18 GB of the old
    4/24 pool); afterwards both were flattened to an identical 1 OCPU / 6 GB
    and the split bought nothing but cost plenty — see
    "Why one VM" below
- **cloud-init** installs Docker + compose, Node 22, and the `bws`
  CLI, and seeds each app dir's `.env.local` with your Bitwarden bootstrap creds
- A **cost budget + alert rules** (`budget.tf`) — Terraform-managed instead of
  a manual console click-through, so it's idempotent and re-appliable. Set
  `budget_alert_email` in `terraform.tfvars` to wire up email alerts at
  50/80/100% actual spend + 100% forecast; the budget itself is always
  created even without an email.
- An **automated kill switch** (`budget-guard.tf`) — real OCI infra (Events
  rule + Notifications topic + Function), not a polling script: at 50% of
  budget it stops both VMs (recoverable), at 80% it terminates them outright.
  Gated behind `function_image` — see "6. Automated kill switch" below.
- A **compartment quota** (`always-free-quota.tf`) — the strongest of the
  three cost-safety layers: a preventive hard cap enforced by OCI itself at
  request time, not a reaction to spend already happening. Blocks
  provisioning anything ANYWHERE IN THE TENANCY (root + every compartment,
  not just this repo's own) beyond the Always-Free A1.Flex (2 OCPU/12GB) +
  E2.1.Micro pool + 200GB block storage. A scheduled drift check
  (`scripts/check-quota-drift.sh`, every 6h) alerts if the live quota is
  ever loosened outside Terraform — see "8. Compartment quota" below.
- **Security alerts** (`security-alerts.tf`) — real-time audit-event alerts
  for security list/NSG changes and compute launch/terminate, to email
  and/or PagerDuty, independent of spend. Gated behind `security_alert_email`
  / `pagerduty_integration_key` — see "9. Security alerts" below, including
  a confirmed live gap: the IAM/credential category is configured but does
  **not** currently fire (use Cloud Guard for that instead, per section 9).

> The deploy workflows now build **arm64** images (via `docker buildx`), matching
> this VM. If you switch to an x86 shape, change `--platform linux/arm64` back
> to `linux/amd64` in `.github/workflows/deploy-*.yml`.

## Why one VM

This used to be two (`light` + `heavy`). No documented rationale for the split
ever existed in this repo; it was a fossil, and it cost more than it bought.

**What the split actually bought:** isolation, and only of the bluntest kind.
No compose file in this repo sets a `cpus:`/`mem_limit:`/`deploy.resources`
constraint, so "which VM" was literally the only thing keeping
`remote-chat-agent` (an agent runner that spawns `claude`/`opencode`
subprocesses) from starving `leetcode-backend`. That is a real concern, but the
right tool for it is a per-container resource limit, not a whole second host.
**If you add nothing else from this section, add those limits** — on one box a
runaway agent can now OOM every other service.

**What it cost:**

- The control plane ran **twice** — `vm-gateway` (3 containers) + `vm-ops-api`
  (3 containers) on both hosts by design, 12 container slots where 6 do.
- A **bug class that only exists with two hosts.** 2026-08-09:
  the (since-deleted) Telegram bot was provisioned onto both VMs; Telegram permits one
  live `getUpdates` consumer per token, so both copies crash-looped for ~3
  hours. `vm-gateway`'s `TS_HOSTNAME` collided across the two hosts for the
  same reason.
- **Three guards built to prevent that recurrence, which had to be kept in sync
  with each other** — `_assert_no_app_overlap` (`main.tf`), `HOST_ALLOWED_APPS`
  (then in vm-ops-api), and a (since-deleted) lint rule that
  existed to keep the first two agreeing. The first is deleted; the second and
  third survive in a reduced form that still catches a blank `HOST_LABEL` and
  single-sided app declarations.
- A **doubled Terraform surface** (`light_apps`/`heavy_apps`,
  `light_ocpus`/`heavy_ocpus`, two break-glass key variables, two `user_data`
  renderings) and **two capacity placements that both had to succeed** — "Out
  of host capacity" is the common A1 failure, which is why
  `scripts/retry-apply.sh` exists.

**What it did not buy:** a surviving second ingress. When `heavy` lost its only
public surface (2026-08-11) it needed a full recreate, and `light` being alive
provided no path in. `budget-guard.tf`'s kill switch also stopped/terminated
both VMs together, so there was no isolation there either.

**Consolidating was quota-neutral.** `always-free-quota.tf` caps
`standard-a1-core-regional-count` at 2 **pool-wide, not per-instance**, so one
2-OCPU instance needed no quota change — the cap file's values are untouched by
this migration.

**Disk arithmetic.** Always Free allows 200 GB of block storage total.
`boot_volume_gbs` (150, the VM) + `dummy_boot_volume_gbs` (50,
`dummy-test-infra.tf`'s permanent e2e box) = exactly 200. Those two shared one
variable before this change, so raising the VM's disk would have silently
force-replaced the e2e box (`boot_volume_size_in_gbs` is ForceNew). The e2e box
doubles as the break-glass rescue host — the VM's boot volume can be attached
to it as a *data* volume, which consumes no extra storage — so taking the full
150 GB does not strand the no-SSH recovery path.

## Prerequisites

- [Terraform](https://developer.hashicorp.com/terraform/downloads) ≥ 1.5
- An Oracle Cloud account (you have this) + an **API signing key**
- A **dedicated** SSH deploy keypair (not your personal key)

## 1. Generate the deploy SSH keypair

```bash
ssh-keygen -t ed25519 -f ~/.ssh/oracle_deploy -N ""
# public half → the VMs (Terraform reads ssh_public_key_path)
# private half → the DEPLOY_SSH_KEY GitHub secret (step 5)
```

## 2. Getting the OCIDs (for terraform.tfvars)

> **Letting an agent apply this on your behalf?** Store the six values below
> in the vault instead (`wf secret set OCI_TENANCY_OCID` etc.; `wf secret list` says
> where each comes from: the dedicated, narrowly-scoped IAM user this should
> authenticate as, never your personal admin login), plus
> `OCI_API_PRIVATE_KEY_PASSPHRASE` if (and only if) you generated the keypair
> yourself with a passphrase rather than via the OCI Console's unencrypted-by-
> default "Generate API key pair" button — then run `wf vm creds
> sync`, which writes `~/.oci/oci_api_key.pem` (chmod 600) and the
> matching block in `terraform.tfvars` for you. Safe to re-run any time the
> vault values change — it only touches its own
> marker-delimited block, leaving the rest of your `terraform.tfvars` (sizing
> overrides, `ssh_public_key_path`, `bws_read_access_token`, ...) untouched.

In the OCI Console:

- **API key + fingerprint + tenancy/user OCIDs:** avatar (top-right) → **My
  profile** → **API keys** → **Add API key** → *Generate key pair*, download the
  private key to `~/.oci/oci_api_key.pem`. The dialog then shows a config preview
  containing `tenancy`, `user`, `fingerprint`, and `region` — copy those.
- **compartment_ocid:** for a personal account the root compartment is fine —
  it's the same value as `tenancy_ocid`. (Or create a compartment under
  **Identity → Compartments** and use its OCID.)

```bash
cp terraform.tfvars.example terraform.tfvars
# edit terraform.tfvars with the values above
```

## 3. Apply

```bash
cd infra/oracle-cloud
terraform init
terraform plan
terraform apply
```

If `apply` fails with **"Out of host capacity"** (common on the free A1 pool),
don't hand-edit `main.tf` — pass a different `-var availability_domain_index=1`
(or `2`) and re-apply, or just run `scripts/retry-apply.sh`, which does this
automatically: it cycles the AD index across attempts within a bounded budget,
and — unlike a bare retry loop — only keeps retrying on that specific error;
anything else (bad OCID, expired auth, quota/`LimitExceeded`) is a real
problem and it stops immediately instead of burning the whole budget on a bug
retrying can't fix.

The script also supports what community tools like
[hitrov/oci-arm-host-capacity](https://github.com/hitrov/oci-arm-host-capacity)
do beyond single-region AD cycling:

```bash
# Also rotate across regions your tenancy is SUBSCRIBED to (Console:
# Governance & Administration -> Region Management) — capacity is
# fragmented per region just like per AD.
scripts/retry-apply.sh --regions eu-frankfurt-1,eu-amsterdam-1,eu-zurich-1

# Last resort: after every region x AD combination is exhausted for
# A1.Flex, fall back to the x86 VM.Standard.E2.1.Micro Always-Free shape.
# Fixed at 1/8 OCPU + 1 GB RAM — likely too small to comfortably run this
# repo's docker + node services, so this gets you *something reachable*,
# not a real substitute. Off by default; opt in explicitly.
scripts/retry-apply.sh --allow-e2-micro-fallback
```

A region-subscription error (tenancy not subscribed to that region) is
detected and skips straight to the next region instead of burning all 3 AD
attempts there; any other unexpected error still aborts immediately per the
above.

Free Tier tenancies are capped at 1 subscribed region (your home region), so
`--regions` with anything else is a no-op there. `--auto-subscribe-regions`
(off by default) has the script try `oci iam region-subscription create` on
a region-not-subscribed error before giving up on it — real tenancy-level
action, so it stays opt-in; safe to enable once you're on Pay-As-You-Go,
which lifts the 1-region cap.

Smaller shape requests succeed more often than larger ones — this repo
already requests the smallest useful A1.Flex split (1 OCPU/6GB per VM); no
further downsizing is planned. Don't confuse this with **idle reclamation**:
once created, Oracle can reclaim an Always-Free instance if, over a 7-day
window, CPU/network/memory utilization all stay under ~20%
([official policy](https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier_topic-Always_Free_Resources.htm)).
A genuinely-used instance (this one, running real services) won't trip that;
the only *legitimate* way around it for a genuinely idle box is converting
the tenancy to Pay-As-You-Go. Don't run fake load (cron'd `stress`, junk
network traffic) to dodge the check — that's gaming the policy, not using it.

## 4. Get the VM hosts

```bash
terraform output vm_host        # ubuntu@<ip>  → VM_HOST
```

There is no `vm_host_heavy` any more. If you are migrating from the two-VM
layout, **delete the now-stale `VM_HOST_HEAVY` repo secret** — left in place it
would quietly point a future workflow at a terminated host.

## 5. Feed them into GitHub Actions secrets

```bash
gh secret set VM_HOST        --repo nuesslerm/mobile-learning-app --body "$(terraform output -raw vm_host)"
gh secret set DEPLOY_SSH_KEY --repo nuesslerm/mobile-learning-app < ~/.ssh/oracle_deploy
gh secret delete VM_HOST_HEAVY --repo nuesslerm/mobile-learning-app 2>/dev/null || true
```

## 6. Populate Bitwarden + deploy

1. In **Bitwarden → Secrets Manager**, create a machine account + project,
   generate its access token, and add the app secrets (keyed by env-var name).
   Full list: `wf secret list`.
2. Put `bws_read_access_token` in `terraform.tfvars` (baked into each VM's
   `.env.local` by cloud-init as `BWS_DEPLOY_SECRETS_READ_ACCESS_TOKEN`)
   **or** write it into `~/<app>/.env.local` on each VM by hand.
3. Trigger a deploy — push to `main` (path-filtered) or run a `deploy-*`
   workflow via **Actions → Run workflow**.

Verify cloud-init finished (it takes a minute or two after `apply`):

```bash
ssh -i ~/.ssh/oracle_deploy ubuntu@<ip> 'docker --version && node --version && bws --version'
```

## 7. Budget alerts + automated kill switch

`budget.tf` creates 10 `ACTUAL`/`ABSOLUTE` alert rules on the budget at fixed
checkpoints — 0.50, 1, 2, 5, 7.50, 10, 12.50, 15, 17.50, and 20 (of
`budget_amount_usd`, currently 20) — plus `budget-guard.tf` wires up the
automated pattern from [Oracle's own writeup](https://blogs.oracle.com/cloud-infrastructure/enforced-budgets-on-oci-using-functions-and-quotas):
each alert firing routes through an Events rule to an ONS Notifications topic
(**email + PagerDuty** — a human/on-call sees every threshold fire regardless
of what the function below does) and, if the Function is deployed, to a
Function that re-queries actual month-to-date spend itself and acts:

- **≥ `budget_guard_stop_threshold_pct`** (default 50% = €10) →
  **stops** both VMs (recoverable — `terraform apply` or a console click
  restarts them)
- **≥ `budget_guard_destroy_threshold_pct`** (default 100% = €20) →
  **terminates** both VMs outright

It re-checks real spend itself rather than trusting which specific alert
fired (Budgets events don't reliably expose that), so it's correct even if
alerts arrive out of order or get coalesced.

**The alert rules + ONS topic + email/PagerDuty notifications work
independently of the Function** — `local.budget_alerting_enabled`
(`var.budget_alert_email != "" || var.pagerduty_integration_key != ""`) is
all they need. Only the Function/Application and the IAM granting it
`manage instance-family` (the actual stop/terminate capability) are gated
behind `function_image`, which doesn't exist until you build and push the
function's container image — this needs a **running Docker daemon**, which
this repo's own sandbox environment usually doesn't have. Until that's done,
you get every alert (10 checkpoints, to email + PagerDuty) with no automated
stop/destroy action — a real gap if you're relying on this as an actual kill
switch, not just notifications. Run this from anywhere with Docker (OCI
Cloud Shell is the easiest — free, browser-based, Docker/oci CLI preinstalled
and already authenticated):

```bash
# One-time: Console -> profile icon -> My profile -> Auth Tokens -> Generate
# Token. This is an Auth Token, NOT your password or the API signing key
# used everywhere else in this repo.
OCIR_AUTH_TOKEN=<token> scripts/deploy-budget-guard-function.sh
```

This builds `functions/budget-guard/` (a small Python function, `oci` SDK +
`fdk`), pushes it to OCIR, and applies `budget-guard.tf` with the resulting
image. Until you run this, everything in `budget-guard.tf` stays uncreated —
a plain `terraform apply` is a no-op for it.

**Known rough edge:** the exact IAM resource-type string for granting the
function read access to cost/usage data (`usage-budgets` in the policy
statement `oracle_identity_policy.budget_guard_fn`) is the one thing I
couldn't verify against a live tenancy from here — Oracle's own docs use
both `usage-budgets` and `usage-report` for cost-read access depending on the
source. If the function's logs show a 403 reading usage data, that policy
statement is the first place to check; IAM failures here are safe (the
function just logs an error and takes no action), never silently wrong.

If the VMs ever get stopped/terminated this way, Terraform's local state
won't know until its next refresh — `retry-apply.sh` runs `terraform apply
-refresh-only` before its own `instances_exist()` check specifically so it
notices and doesn't think a terminated VM still exists.

## 8. Compartment quota

`always-free-quota.tf` is applied unconditionally (no gate, no Docker
dependency, live from the first `terraform apply`) — it's the cheapest and
strongest of the three cost-safety layers here, because it's **preventive**:
OCI itself rejects a request that would exceed it, before anything gets
created, rather than reacting after the fact like `budget.tf`'s email alerts
or `budget-guard.tf`'s stop/terminate function.

Pattern: `zero <family> quotas` blocks everything in that resource family by
default, then `set <family> quota <name> to <N>` allowlists exactly the
Always-Free resources this repo uses:

- `compute` (fixed BM/VM shape counts) — zeroed entirely, not used
- `compute-core` — zeroed, then `standard-a1-core-regional-count` (A1.Flex
  OCPUs) and `standard-e2-micro-core-count` (E2.1.Micro cores) allowlisted
- `compute-memory` — zeroed, then `standard-a1-memory-regional-count`
  allowlisted
- `block-storage` — zeroed, then `total-storage-gb` capped at 200

The exact quota/resource-type names were verified against this tenancy's own
`oci limits definition list --compartment-id <tenancy> --service-name
compute` output, not guessed — if you ever need to redo this for a
different resource, that command (and `--service-name block-storage`) is
the source of truth, not a web search.

One caveat: `standard-e2-micro-core-count` and `total-storage-gb` are
**AD-scoped**, not regional — Oracle's compartment quota engine allocates
these per-AD, so in a multi-AD region the effective cap would be `N ×
(AD count)`, not exactly `N`. Exact for this repo's single-AD `eu-frankfurt-1`
today; would need a `where request.ad = ...` condition per AD to stay exact
in a multi-AD region. `standard-a1-core-regional-count` and
`standard-a1-memory-regional-count` don't have this issue — they're
genuinely region-scoped.

Raising `vm_ocpus`/`vm_memory_gbs`/etc. beyond what this quota allows will
make `terraform apply` fail with a quota-exceeded error — that's the point;
bump `always_free_a1_ocpu_cap` (etc.) in the same commit if you deliberately
want to size past the free tier (which then starts actually costing money —
worth pairing with a `budget_amount_usd` increase too).

**Scoped `in tenancy`, not just this repo's compartment.** The statements
above say `in tenancy`, not `in compartment id <this compartment>` — this
was originally compartment-scoped, which left a real gap: the credential
actually used for all Terraform/CLI work here is a personal admin account
(confirmed via Audit's `principal-name` on live API calls), not a
narrowly-scoped service user, so it has tenancy-wide permissions and could
have created paid resources in the root compartment (or any other
compartment) completely unrestricted by a quota pinned to just one
compartment. Verified live: a launch attempt aimed directly at the root
compartment now also gets rejected with the same `QuotaExceeded` error.
Only do this if you've confirmed (as we did) that no other resource-bearing
compartment exists in the tenancy that this cap could unexpectedly disrupt
— compute/storage quotas don't touch unrelated resource families (IAM,
etc.), but they would clamp down on any other compute/storage anywhere
else in the tenancy too.

**Catching quota tampering itself.** Limits/Quotas doesn't emit Events at
all (see security-alerts.tf's "Known gap #1"), so a credential widening
this quota directly via `oci limits quota update` — bypassing Terraform
entirely — can't be caught in real time. `scripts/check-quota-drift.sh` is
the substitute: it runs `terraform plan -target=oci_limits_quota.always_free_only`
and treats any diff as drift, publishing an alert directly to the
security-alerts ONS topic if so (bypassing the Events pipeline the same
way, since that's the only reliable delivery path proven for this kind of
synthetic/non-Audit-sourced event). Scheduled to run every 6 hours via a
Routine (`trig_01UPcBLeJcbeh6YisXZ6mwj6`) that re-fires this same check —
adjust the cadence with `update_trigger` if 6 hours is more/less than you
want.

`scripts/test-quota-drift-alert.sh` verifies the whole loop live: it bumps
`standard-a1-core-regional-count` by 1 directly via the OCI API (the exact
attack this guards against), confirms `check-quota-drift.sh` detects it and
alerts, then reverts via `terraform apply` in a trap — guaranteed to run
even if the script is interrupted, so no elevated quota is ever left in
place. Ran successfully live: drift detected, alert published, revert
confirmed via a follow-up `check-quota-drift.sh` run showing no drift.

**Attribution:** the alert names who actually made the change, not just
"drift was detected." Limits/Quotas doesn't produce Events, but an
`UpdateQuota`/`DeleteQuota` call is still logged in the general Audit trail
(confirmed live) with the real caller's identity —
`check-quota-drift.sh` looks that up (correlating via the quota OCID in
the audit record's `request.path`) and puts the real principal name, IP,
and client (e.g. `Oracle-GoSDK/... Oracle-TerraformProvider/...` vs. a
bare `oci`-CLI user-agent) into the alert, falling back to an explicit
"unknown, Audit hasn't caught up yet" rather than fabricating an identity
if no matching record is found within the lookback window. Verified live:
an alert correctly attributed to the real account and IP that made the
test change, not a generic placeholder.

## 9. Security alerts

`security-alerts.tf` watches the OCI Audit log in real time (via an Events
rule) for exactly the actions a breached API key — or a bug — could take
that nothing else here catches, independent of whether it ever costs money.

- **Security list / NSG changes** — this repo only ever touches the default
  security list once, via `main.tf`; any change outside a deliberate
  `terraform apply` is worth a look. **Confirmed working live**: a manual
  test NSG create/delete correctly delivered and paged. **Confirmed
  limitation**: the raw event OCI delivers for this event type has no
  `data.identity` at all (verified against a real incident's full payload)
  — `pagerduty-transform.js` falls back to
  `data.definedTags['Oracle-Tags'].CreatedBy` (the resource's original
  creator, auto-tagged by OCI), labeled as such rather than presented as
  certain, since it's wrong for a later update/delete by someone other
  than the creator. True per-action attribution for event types missing
  `identity` would need server-side enrichment (an OCI Function querying
  Audit, like `check-quota-drift.sh` does) — not built, since deploying a
  Function needs Docker, same blocker as `budget-guard.tf`.
- **Compute launch/terminate** — only `retry-apply.sh`, `terraform destroy`,
  or `budget-guard.tf`'s function should ever do this. A launch the
  compartment quota (section 8) rejects does **not** fire this — confirmed
  via `scripts/test-quota-block.sh`, OCI only emits these for a request it
  actually starts acting on, not one synchronously rejected before any work
  begins — so this category only covers a launch that actually proceeds
  (not independently verified live for a *successful* launch, but uses the
  same delivery path already confirmed working for NSG changes above).

**Configured, but confirmed NOT currently working** (see "Known gap #2"
below): `UploadApiKey`, `CreateAuthToken`, `CreateCustomerSecretKey`,
`CreateUser`, `AddUserToGroup`, `CreatePolicy`/`UpdatePolicy`/`DeletePolicy`,
etc. — new/changed credentials, users, groups, and policies. These
eventTypes are still in the rule's condition, but a live A/B test shows
they never actually deliver in this tenancy. **Use Cloud Guard for this
category instead** (see below) until/unless this changes.

Set either or both in `terraform.tfvars`:

```hcl
security_alert_email      = "you@example.com"
pagerduty_integration_key = "..."
# Only needed on PagerDuty's EU service region — default is the US host.
# Check your account's own URL: <subdomain>.eu.pagerduty.com means EU.
pagerduty_events_domain = "events.eu.pagerduty.com"
```

Both `security_alert_email` and `pagerduty_integration_key` are optional
independently, but the whole rule/topic/policy only gets created once at
least one is set (`local.security_alerts_enabled`) — a plain `terraform
apply` is a no-op for this file otherwise, same pattern as `budget-guard.tf`.

`pagerduty_integration_key` and `pagerduty_events_domain` are also
vault-backed, same as the rest of this repo's OCI credentials:
`PAGERDUTY_INTEGRATION_KEY` / `PAGERDUTY_EVENTS_DOMAIN` are registered in
`clis/secrets/src/data/registry.json` (`oracle-terraform` group, both
optional) — `wf secret set PAGERDUTY_INTEGRATION_KEY` then
`wf vm creds sync` writes it into terraform.tfvars for you,
same marker-delimited block as the OCI_* values. Only store the bare
integration **key**, never the full endpoint URL — Terraform builds that
itself from the key + domain, and the URL just embeds the same key a
second time (redundant, not an extra layer of protection).

**Email**: OCI sends a confirmation link to `security_alert_email` on
create — same as every other ONS email subscription in this repo. Nothing
arrives until it's clicked.

**PagerDuty setup** (you'll need to do this on the PagerDuty side first):

1. In PagerDuty, open (or create) the service you want alerts to page, go to
   its **Integrations** tab, and add a new integration.
2. Pick **Custom Event Transformer** as the integration type — **not**
   "Oracle Cloud Infrastructure Monitoring". The Monitoring integration only
   creates incidents from alarm-format messages and silently drops anything
   else; Events/ONS payloads aren't in that format, so alerts would never
   arrive.
3. Copy the resulting **Integration Key** and set it as
   `pagerduty_integration_key` in `terraform.tfvars`. (Terraform builds the
   full endpoint URL — `https://<pagerduty_events_domain>/integration/<key>/enqueue`
   — itself; you only need the key, never the full URL.)
4. Check which **service region** your PagerDuty account is on — look at
   its own URL. `<subdomain>.eu.pagerduty.com` means EU: set
   `pagerduty_events_domain = "events.eu.pagerduty.com"` in
   `terraform.tfvars`. Anything else (e.g. `<subdomain>.pagerduty.com`) is
   the US region and needs no override — the variable already defaults to
   `events.pagerduty.com`. Getting this wrong doesn't error; the request
   just never reaches your account and alerts silently never arrive.
5. `terraform apply`. OCI posts a confirmation payload to that same URL,
   which the Custom Event Transformer integration confirms automatically —
   no manual click needed, unlike email.

**On the EU service region specifically:** `security-alerts.tf`'s
subscription uses OCI's `CUSTOM_HTTPS` protocol, not `PAGERDUTY` and not
plain `HTTPS` — both of those hardcode server-side URL validation to the
literal prefix `https://events.pagerduty.com/integration/` and reject
anything else with a 400, including the EU host (verified live against
this tenancy, both fail identically). `CUSTOM_HTTPS` is the one protocol
type that skips that check while still delivering a plain HTTPS POST,
which is all a Custom Event Transformer integration needs. If you ever see
`400-InvalidParameter, Unexpected URL value` applying this file, this is
why — don't "fix" it by switching back to `PAGERDUTY`/`HTTPS`.

**Verifying it actually delivers:** `scripts/test-security-alert.sh` fires a
real, harmless test — it creates then immediately deletes a throwaway OCI
auth token on the `agent-terraform` user (every OCI user can always manage
their own credentials regardless of IAM policy, so this works even though
that user's own policy is otherwise scoped to compute/network/budget-read
only). It polls this tenancy's Audit log for both `CreateAuthToken` and
`DeleteAuthToken` to confirm the events actually happened, which is as far
as it can verify from here — it can't see whether PagerDuty/email actually
received or rendered the alert, so check your phone/inbox for up to 2
pushes after running it:

```bash
scripts/test-security-alert.sh                       # 180s default timeout
scripts/test-security-alert.sh --timeout-seconds 300  # if Audit is lagging
```

Safe to re-run any time you change `security-alerts.tf` or want to confirm
the pipeline's still healthy — no lingering credential, no cost.

`scripts/verify-infra-state.sh` is the read-only counterpart — no live
fire, just confirms the quota/alert/budget resources that should exist in
the tenancy actually do (`ACTIVE`, not just present in Terraform's state).
Run it any time as a cheap first check before reaching for a live-fire
script.

`scripts/test-quota-block.sh` verifies `always-free-quota.tf` (section 8)
actually blocks non-free provisioning, live: the Always-Free A1.Flex pool
(2 OCPU regional cap) is already fully held by this repo's own single VM
instances — a **stopped** instance still holds its allocated OCPU/memory
quota in OCI, it isn't released until terminated — so attempting a 3rd
A1.Flex instance should get rejected before anything is created. If it
ever unexpectedly succeeds, the script terminates what it created rather
than leaving a stray billable resource.

**That's your generic incident, explained:** the screenshot showing an
Oracle-branded "Subscription confirmed" page is **not** the security
alert — it's OCI's own one-time notice that the ONS subscription reached
`ACTIVE`, sent automatically the moment `terraform apply` created it,
unrelated to any actual event firing. The *real* incident content — what
you see in PagerDuty for an actual `CreateAuthToken`/etc. — looked generic
for a different reason: **Custom Event Transformer integrations require
you to write your own JavaScript transform script**; without one, PagerDuty
falls back to a bare-bones default that doesn't extract anything from the
payload. `pagerduty-transform.js` (repo root of `infra/oracle-cloud/`) is a
real transform script that parses OCI's actual event JSON (delivered
as-is in the POST body — metadata like message type lives in
`X-OCI-NS-*` headers, not wrapped into the body) into a proper summary:
*"`<eventType>` by `<principal>` in `<compartment>` (`<resource>`)"*, plus
full event details. Paste it in:

```
PagerDuty -> Services -> (the service) -> Integrations tab
  -> the Custom Event Transformer integration -> Edit Transformer
  -> replace the script body with pagerduty-transform.js -> Save
```

Deployed at: https://mobile-learning.eu.pagerduty.com/services/PDZA7DG/integrations/P9N891K

This can't be applied via Terraform or the OCI CLI — it only exists inside
PagerDuty's own UI code editor, so it isn't automatically kept in sync;
`pagerduty-transform.js` in this repo is the source of truth to re-paste
if it's ever changed. Verify it worked by re-running
`scripts/test-security-alert.sh` and checking the resulting incident's
summary — it should read like the sentence above, not empty/generic.

**Known gap #1:** `oci limits quota update|delete` (i.e. someone tampering
with `always-free-quota.tf`'s own guardrail) doesn't fire an Event at all —
Limits/Quotas isn't one of the services OCI's Events reference lists as an
event producer, verified directly against that reference rather than
assumed.

**Known gap #2 — CONFIRMED LIVE, and the bigger one:** the IAM/identity
category (new API keys, auth tokens, users, groups, policies) **does not
currently alert at all**, despite being configured to. Verified with a
controlled A/B test on the live tenancy: creating+deleting an IAM group and
an auth token produced zero deliveries (both the ONS topic's own
`PublishedMessagesCount` metric and the Events service's own "Events
Emitted" chart stayed at zero), while creating a throwaway Network Security
Group — a core Networking event, not identity-plane — delivered correctly
and paged as expected, with the rule/topic/subscription/transform script
completely unchanged between the two tests. This tenancy uses **Identity
Domains** (confirmed via IDCS/SCIM-shaped fields in the raw Audit payload,
e.g. `idcsCreatedBy`, `urn:ietf:params:scim:schemas:oracle:idcs:authToken`)
— identity-plane events apparently aren't wired into the classic Events
pipeline for an Identity-Domains tenancy, even though Oracle's own Events
reference lists "IAM" as a producer and core-service (compute/network)
events clearly do work. The IAM eventTypes are left in
`security-alerts.tf`'s condition in case Oracle fixes this — harmless if
so, they just never match today — but don't rely on this file for
credential/user/policy changes right now.

**OCI Cloud Guard** is the mitigation for both gaps: it's free, tenancy-wide,
and its IAM Activity Detector reads Audit directly rather than going
through the Events pipeline, so it isn't necessarily subject to gap #2 —
given this finding, it's the thing to actually turn on for credential/IAM
monitoring, not just "defense in depth" as originally framed here. It's a
one-time console toggle (Security → Cloud Guard → Enable), not Terraformed
here since its managed detector recipes aren't worth hand-rolling when the
built-in ones already cover this for free.

**Second known gap, discovered by `scripts/test-quota-block.sh` (not
assumed):** a compute launch that `always-free-quota.tf` rejects does
**not** fire `launchinstance.begin`/`.end` at all — OCI only emits these
for a request it actually starts acting on, not one it rejects
synchronously before any work begins. So the quota genuinely stops a
non-free launch, but security-alerts.tf's compute category can't tell you
it was attempted — you'd only notice via the failed API response itself
(e.g. in `retry-apply.sh`'s own logs) or by checking Audit by hand. Cloud
Guard is the same mitigation as above: its Activity Detector reads Audit
directly rather than going through Events, so it isn't subject to this gap.

## 10. vm-ops-api (PoC): letting a sandboxed agent verify VM state over HTTPS

A sandboxed coding agent with only HTTPS egress (no SSH, no raw TCP, no
WebSocket-upgrade tunnels — a common restriction for agent sandboxes) can't
SSH into these VMs, and can't reach their Tailscale-only app ports either
(Tailscale needs a UDP tunnel). `vm-ops-api/` is a small proof-of-concept
answering "can it still verify/drive VM state at all?": a bearer-token-gated
HTTP API, fronted by Caddy for automatic Let's Encrypt TLS on port 443, with
no dependency on owning a domain name (uses the free `sslip.io`/`nip.io`
trick — `<ip>.sslip.io` resolves to that exact IP via public DNS, which
satisfies Let's Encrypt's HTTP-01 challenge).

**Why not just open 443 and trust a firewall rule?** The calling agent has
no fixed, knowable source IP — it sits behind a shared, rotating egress
proxy. An IP-based OCI security-list allowlist literally cannot include it.
So `main.tf`'s security list deliberately allows `0.0.0.0/0` on 80/443 (80
is ACME HTTP-01 challenge traffic only), and the real access control is the
bearer token (`VM_OPS_API_AUTH_TOKEN`, constant-time-compared in `backend/vm-ops-api/core/src/domain/auth.rs`) —
same posture as any other public API secured by a credential rather than a
network boundary.

**How code changes reach the VM without SSH.** CI (`.github/workflows/deploy.yml`)
ships vm-ops-api as its first deploy leg: the job builds and pushes `vm-ops-api-rs` and the
watchdog as `sha-<short>` on the runner's builder sidecar, then
`wf ship vm-ops-api --ref <sha> --redeploy-only --tag sha-<short> --wait-serving`
calls `POST /redeploy?ref=<sha>&tag=sha-<short>`; vm-ops-api (the Rust service, image
`vm-ops-api-rs`) builds nothing, it proves the pushed image as a throwaway container and
swaps itself in through a detached sibling: it stages and
checks the new compose files, keeps the old ones as `.prev`, runs
`docker compose up -d --wait` (so a new image that never turns healthy fails
too), puts the old files back and brings the old stack up again at the tag that
was running if that fails (`restored` in `wf ship --status`'s history line says
how that went), and writes `IMAGE_TAG=sha-<short>` into `.env` only once it
passed. The leg is done when `/health` reports the commit. `latest`, which a
cloud-init or watchdog recreate pulls, moves in CI once the leg serves the commit
(a push to dev): `wf ship promote vm-ops-api --tag sha-<short>` points
`latest` of `vm-ops-api-rs` and `watchdog` at that tag, registry to registry
(`docker buildx imagetools create`), with no vm-ops-api route. No SSH in that loop at all
— `deploy.sh` only does the VM's *first-ever* setup (or a compose/Caddy config
change).

Initial deploy (needs real SSH — run from a machine with network access,
not the sandbox that authored this):

```bash
# Add VM_GATEWAY_AUTH_SEED (e.g. openssl rand -hex 32) directly in Bitwarden
# Secrets Manager, NOT via `wf secret set` (that command is macOS-keychain
# only — it never reaches the vault the VM actually reads from). Grant it to
# a project the read-only "Deploy Secrets" account can see — the VM only
# ever holds that token, never the write-capable one, so restricting this
# to an admin-only project would make the VM unable to resolve it at all.
cd infra/oracle-cloud/vm-ops-api
# the images deploy.sh pulls are the ones CI pushed: a push to dev (docker.io/nuesslerm/vm-ops-api-rs:latest moves once a build served)
VM_HOST=ubuntu@<ip> HOST_LABEL=main OPS_API_DOMAIN=<ip>.sslip.io ./deploy.sh
curl -H "Authorization: Bearer <token>" https://<ip>.sslip.io/health
```

Ongoing code changes (no SSH, fully drivable from a sandbox):

```bash
wf ship vm-ops-api --ref <sha> --redeploy-only --tag sha-<8> --wait-serving   # what CI runs after it pushed the tag; POST /redeploy?ref=<sha>&tag=
wf vm ops health                                # confirm it took
```

**Endpoints** (all require `Authorization: Bearer <VM_GATEWAY_AUTH_SEED>`):

- `GET /health` — liveness + which host answered (`main`, the only one).
- `GET /services/:app/ps` — container name/state/status/image for `:app`
  (must exactly match one of `KNOWN_APPS` in `backend/vm-ops-api/core/src/domain/apps.rs` — an unlisted name
  404s, it never reaches Docker). Read-only.
- `GET /services/:app/logs?tail=200` — stdout+stderr for every container
  matching `:app`, tail capped at 1000 lines. Read-only.
- `GET /services/:app/logs?follow=true[&container=<name>]` — same
  endpoint, live mode: streams stdout+stderr as plain text (chunked
  response) until the caller disconnects or the container stops, instead of
  returning a fixed snapshot. Multi-container apps (e.g. the app + its
  tailscale sidecar) need `&container=<name>` to pick one — interleaving
  several live streams into one response isn't useful, so an ambiguous
  request without it gets a 400 listing the container names. Read-only.
- `POST /redeploy/smoke?ref=<commit>&tag=<tag>` and `POST /redeploy/smoke/stop` — the smoke test of vm-ops-api's own image, a
  step of its own that `wf ship` runs between the build and the swap. A foreground sibling pulls the image and runs it once as a
  throwaway container on the spare port 18999 in the netns of the live one, passes `--healthcheck` from inside and is **left
  running** (`self_smoke_sibling_args`); `wf` then sends it a signed `GET /health` over the tailnet on that port, and the stop route
  removes it. A failed test answers `502` with the container's log, records `redeploy.failed` and swaps nothing. Every managed app
  has the same pair, `POST /services/<app>/smoke?tag=&ref=` and `/services/<app>/smoke/stop` (an app with no smoke test answers
  200 saying so), and `wf` asks the container's own `/health` on the spare port over the tailnet.
- `POST /redeploy?ref=<commit>&tag=<tag>` — `wf ship vm-ops-api` has already built and pushed `vm-ops-api-rs` and `watchdog` at `<tag>`
  (a waited job; this service builds nothing) and smoke-tested them. It answers `202`
  and launches a detached **sibling** container (a separate
  `docker run`, not a child process of this one) to run `docker compose
  pull|up` — a child process would share this container's cgroup/PID
  namespace and get killed partway through the very recreate it's
  performing; a sibling reached over the same socket is unaffected by that.
  See `backend/vm-ops-api/core/src/application/self_redeploy.rs` and
  `backend/vm-ops-api/core/src/application/scripts/redeploy.rs`'s `self_redeploy_sibling_args`.
- `POST /services/:app/redeploy?tag=<tag>` — the same mechanism, generalized
  to redeploy one of the **four real production apps** instead of
  `vm-ops-api` itself: pulls `<namespace>/<app>:<tag>` and runs
  `pull → down → up -d` (mirroring the app's own deploy script
  exactly) via a sibling container mounting that app's real host directory.
  Unlike the self-redeploy above, `vm-ops-api`'s own container isn't being
  torn down here, so there's no self-destruction hazard — the sibling runs
  in the **foreground** and the response is the real, synchronous
  pull/down/up exit code + output (`200`/`502`), not a blind `202`. The same
  "can only select an already-published tag" invariant holds (these images
  come from `build-images.yml`'s CI, not this token), but be clear-eyed that
  a wrong or rolled-back tag here means real user-facing downtime on an
  actual service, not an inert test container restarting — that's an
  accepted operational risk of this endpoint existing, not an oversight.
  Takes an optional `&ref=<branch>` selecting which git ref the app's
  compose file, `.env.defaults` and `wf` bundle are fetched from (default
  `dev`; a `sha-<commit>` tag implies that commit). **A `ref` is not
  remembered**: it selects what that one redeploy provisions, and the next
  redeploy, a watchdog recreate and a cloud-init recreate all use the default.
  Deploys come from `dev` (CI passes the pushed commit's sha). Fresh-VM
  provisioning has its own equivalent: Terraform's `var.provision_ref`,
  threaded into cloud-init.
  Refuses with `409 { error: 'app not declared for this host' }` if `:app`
  isn't in this VM's `KNOWN_APPS` entry (`backend/vm-ops-api/core/src/domain/apps.rs`, mirrors
  `main.tf`'s `local.vm_apps`; edit both together). Originally added after
  the 2026-08-09 incident where the since-deleted Telegram bot (a Telegram
  long-polling bot, only one live `getUpdates` consumer allowed per token) got
  provisioned onto both VMs at once and both copies crash-looped for ~3 hours.
  With one VM that cross-host failure is structurally impossible; the check
  remains because it still fails closed on a blank/unrecognized `HOST_LABEL`
  (which means cloud-init did not write the app `.env` correctly) and still
  catches an app added to only one of the two declarations.
`ps`/`logs` talk to the Docker Engine API directly over a mounted
`/var/run/docker.sock` (no `docker` CLI needed for those two). Both
`/redeploy` endpoints shell out to `docker compose pull|down|up` (the image
includes `docker-cli`/`docker-cli-compose`) rather than hand-rolling
container recreation via the raw API — compose already gets network
reattachment and container naming right, which is easy to get subtly wrong
reimplementing it. `:ro` on the socket mount doesn't restrict which Docker
API calls are possible over it, including creating/starting a sibling
container (daemon-level access, not mount-level, confirmed live) — the
hardcoded endpoint whitelist is the real boundary, not the mount flag.

```bash
curl -H "Authorization: Bearer <token>" https://<ip>.sslip.io/services/remote-chat-agent/ps
curl -H "Authorization: Bearer <token>" "https://<ip>.sslip.io/services/remote-chat-agent/logs?tail=50"
curl -X POST -H "Authorization: Bearer <token>" "https://<ip>.sslip.io/redeploy?ref=<sha>"
curl -X POST -H "Authorization: Bearer <token>" "https://<ip>.sslip.io/services/remote-chat-agent/redeploy?tag=sha-abc1234"
```

- **The one deliberate exception to "never a generic exec endpoint" is
  `POST /services/agent-shell/exec-script` (+ its `/:jobId` status and
  `/:jobId/cancel` siblings).** It runs a caller-supplied script body, but
  only inside `agent-shell` — the one container whose own purpose already
  accepts arbitrary code execution as its threat model (its README: an
  interactive shell for `claude`/`codex`/`opencode`/`wf`; SSH access there
  already grants exactly this). No other app gets this. Async job model
  (start → poll → cancel), not one blocking call with a hard timeout — see
  `agent-shell/README.md`'s "Ad-hoc script execution" section for the full
  contract and a worked example.

The rest of the route set (`backend/vm-ops-api/rs/src/api/mod.rs` `app`): the
`GET /opencode-log[/api/runs[/:id]]` dashboard (public, no token),
`POST /opencode-log/:runId/{start,line,finish}`,
`GET`/`POST /cooldowns` (versioned pool-cooldown ledger, a stale write 409s),
`GET /redeploy/status`, `GET /stats`, `POST /services/:app/retire`,
`POST /services/orca-serve/pair` (one fixed `docker exec` sibling; the reply carries the pairing link, a credential never logged; `wf vm ops pair orca-serve`),
`GET /services/agent-shell/{artifact,vm-ledger/:name}`,
`POST /agent-shell/credentials` (vnc-auth-capture's credential relay).
`POST /drill?action=stop|freeze` (the second-door drills; the watchdog recovers the container) takes the signed token only; the raw
token is `401`. There is one vm-ops-api, the Rust service, answering on 8080; a broken one is fixed or the VM is
recreated with Terraform, never covered by a standby.
Every unknown path answers `401` without a token and `404` with one; the
dev-bridge routes of metro-runner are gone (22 Oct), so they are unknown paths. vm-ops-api deploys nothing on its own: there is no trunk
poll; every deploy is a CI leg.

### Building the 4 production apps locally when CI can't

`.github/workflows/build-images.yml` is the only thing that ever pushes new
images for the real backend apps (`leetcode-backend`,
`remote-chat-agent`, `leetcode-auth-cookie-capture`)
— unlike `vm-ops-api`, they had no manual build+push fallback, so a CI outage
(e.g. a GitHub Actions billing issue) means no new image for any of them,
indefinitely. `scripts/build-and-push-image.sh` (repo root) is that fallback:
same Dockerfile, same repo-root context, same `sha-<short>`/`latest` tag
scheme as CI, buildable from any machine with Docker — including this
sandbox, which needed a sandbox-only, non-committed CA-trust patch (see the
script) to get past the proxy's TLS interception during `npm`/`apt` fetches.

```bash
./scripts/build-and-push-image.sh leetcode-backend
# -> pushes nuesslerm/leetcode-backend:sha-<short> and :latest
```

Docker's own layer cache (not a rewritten Dockerfile) is what makes repeat
builds on the same daemon incremental — only layers after a real source
change get rebuilt; the expensive toolchain/corepack layer stays cached.

### First-time provisioning via `/services/:app/redeploy`

None of the four apps had ever actually been deployed to a VM
(confirmed by direct inspection) — `/services/:app/redeploy` used to assume
`~/<app>/docker-compose.*.yml` + `.env` already existed there, so it could
never do anything useful for a never-deployed app. It now bootstraps that
directory itself the first time: if the compose file is missing, the sibling
container fetches that app's compose file + `.env.defaults` + the `wf` CLI
bundle from a **pinned `main`-branch ref** (never from the request — same "no
path from a request to novel content" rule the `tag` param already follows),
then builds `.env` exactly like the app's own deploy script does,
sourcing `~/<app>/.env.local` for the Bitwarden read-only bootstrap
credential — which `cloud-init.sh.tftpl` already seeds into every app
directory unconditionally at VM-creation time, deployed or not.

This repo is private, so an unauthenticated `raw.githubusercontent.com` fetch
404s (confirmed directly) — provisioning needs **`VM_PROVISION_GITHUB_TOKEN`**,
a fine-grained PAT scoped to **`Contents: Read` only**, on this one repo, set
via `wf secret set VM_PROVISION_GITHUB_TOKEN` (see the secrets registry entry
for the exact GitHub UI steps). Deliberately much narrower than `GITHUB_TOKEN`
(remote-chat-agent's clone/push/PR credential) — provisioning has no business
holding write/PR/actions scope. Without it, provisioning a not-yet-deployed
app fails with a clear `GET ... -> 404`; redeploying an already-provisioned
app is unaffected.

**Adding a brand-new app after the VMs already exist** hits one more gap:
`cloud-init.sh.tftpl`'s `.env.local` seeding loop only runs once, at
VM-creation time, over the app list it knew about *then* — an app added
later (like `vm-gateway`) never gets that file, so its first provisioning
attempt fails with `Bitwarden Secrets Manager list failed — Missing access
token` (confirmed live). Since the BWS read token + region are account-wide,
not per-app, `buildAppRedeploySiblingArgs` now falls back to copying
`vm-ops-api`'s own `.env.local` (mounted read-only into the sibling
alongside the target app's directory) whenever the target app doesn't have
one yet — `vm-ops-api` is always present in cloud-init's seed list, so it's
a safe copy source for any newer app. `cloud-init.sh.tftpl`'s own app list
was also updated to include the new app, so a *fresh* VM build seeds it
directly and never needs the fallback at all — the sibling-copy is what
covers the two VMs that already existed when the app was added.

### `DEBUG_MODE` — an on/off switch for easier live debugging

A single `DEBUG_MODE=true` value, dropped into any app's `.env.local` on the
VM (the per-VM override file cloud-init already seeds and every redeploy
already re-sources — no Terraform apply / VM recreate needed to flip it,
just a redeploy), currently widens three things:

- **`vm-gateway`'s proxy registry** — normally only `vm-ops-api` is
  reachable through the gateway (the operational surface: health, logs,
  redeploy). With `DEBUG_MODE=true` on `vm-gateway` itself, every other
  service that joins the tailnet also becomes reachable through the same
  public Funnel domain, same bearer token, same forwarding code:
  `leetcode-backend`, `remote-chat-agent`,
  `leetcode-auth-cookie-capture`.
  This is deliberately opt-in rather than always-on — these apps were never
  meant to be reachable from the public internet by default, matching the
  single-public-surface decision this repo already made. `GET /health` on
  `vm-gateway` reports both `debugMode` and the current `reachable` app
  list, so it's never ambiguous which state a VM is in.
- **Heartbeat interval** — every backend's `health.heartbeat`
  (`service_kit::heartbeat`) defaults to once every 10 minutes; with
  `DEBUG_MODE=true` on that specific app, it drops to every 15s, useful for
  watching a suspected crash-loop without waiting out a long interval.
- **`vm-ops-api`'s log-tail cap** — `GET /services/:app/logs` normally caps
  at 1000 lines; with `DEBUG_MODE=true` on `vm-ops-api` itself, it rises to
  10,000. Its own `GET /health` reports `debugMode` too.

Deliberately NOT gated behind this flag: anything resembling arbitrary
command execution against a production app container. With one narrow
exception, none of vm-ops-api's endpoints are a generic exec path — that's a security boundary, not a
debugging inconvenience, and stays true regardless of `DEBUG_MODE`. The
one exception, `POST /services/agent-shell/exec-script` (see section 10
above), is scoped to the single container whose own purpose already
accepts arbitrary code as its threat model — it is not, and must never
become, a pattern `DEBUG_MODE` extends to any other app.

## 11. `wf vm terraform` — narrow Terraform operations

`wf vm terraform` (`clis/deploy/src/infra.rs`) keeps a fixed,
argv-only menu, safe to pre-approve as a Bash allow-rule. An instance reboot
has no command of its own: `wf vm recover` soft-reboots, then hard-resets, when
vm-ops-api stays dark.

| Command | Recovery |
|---|---|
| `wf vm terraform apply <dir> <compartment-id>` (free-tier-guarded) | automatic: a non-converged post-apply plan pushes the pre-apply state back |
| `wf vm terraform exec <dir> -- <args>` | unguarded passthrough (plan, output, state list, init, import, destroy) |

OCI Run Command is not offered on this image/shape (established 2026-08-11),
so the former Run-Command break-glass commands (`oci service recover|rollback|
stop|restart`, `instance containers-status`, plugin toggles) were removed in
2026-10. Break-glass today: join the tailnet
(`docs/solutions/infra/sandbox-joins-tailnet-instead-of-funnel.md`), `wf vm recover`,
or the rescue-host boot-volume swap (`dummy-test-infra.tf`).

**Free-tier guard (hard block, no override):** `tf apply` checks every
planned create/resize against the Always-Free A1.Flex pool before touching
anything. It is a second layer under `always-free-quota.tf`'s OCI-enforced
compartment quota (§8).

## Adopting resources you already created by hand (`terraform import`)

If you clicked some resources together in the console first, bring them under
Terraform instead of duplicating them:

```bash
terraform import oci_core_instance.main <instance-ocid>
terraform import oci_core_vcn.main      <vcn-ocid>
# ...then `terraform plan` and reconcile config to match, before any apply.
```

## Remote state (OCI Object Storage) — and the one-time migration still owed

State lives in the `mobile-learning-tfstate` bucket (private, **object
versioning enabled**, Oracle-managed AES-256 at rest), reached through OCI's
S3-compatible endpoint — see the `backend "s3"` block in `versions.tf`.

Always drive terraform through `wf`, never bare:

```bash
wf vm terraform exec  <repo>/infra/oracle-cloud -- plan
wf vm terraform exec  <repo>/infra/oracle-cloud -- state list
wf vm terraform apply <repo>/infra/oracle-cloud <compartment-ocid>   # guarded
```

`wf vm terraform` injects the backend credentials from the vault and the two env
vars OCI's compat layer needs, on **every** terraform shell-out — including the
`state pull`/`state push` pair that `tf apply`'s auto-rollback depends on. A
bare `terraform` reads state fine and then fails
`501 NotImplemented: AWS chunked encoding not supported` the first time it tries
to *write*, so the failure surfaces at the worst possible moment.

For everything but a guarded apply — `plan`, `init`, `import`, `workspace`,
ad-hoc flags — use the passthrough, which carries the same credentials but is
**unguarded** (no free-tier check, no rollback):

```bash
wf vm terraform exec <repo>/infra/oracle-cloud -- init
```

**Why this exists.** State used to live only on whichever machine last ran
`apply`, gitignored with no remote. A fresh clone or a sandboxed agent session
therefore could not plan or apply at all without re-importing every resource by
hand. On 2026-08-11 that turned a `VM_GATEWAY_AUTH_SEED` rotation into a full
recreate of both VMs, because no session could run a targeted apply. Bucket
versioning is the second half of the lesson: every previous state file is
retained, so a bad write is recoverable rather than terminal.

> **⚠️ The remote state is currently EMPTY — do not `apply` against it yet.**
> The bucket and backend are wired and verified, but no resources have been
> adopted into it. Running `apply` now would plan to **create a second copy of
> the entire stack** (VCN, subnet, gateway, route table, security list, both
> instances) alongside the live one.
>
> The fix is a one-time migration, run **from the machine that still holds the
> real local `terraform.tfstate`** (a laptop that has run `apply` before):
>
> ```bash
> # detects local state, prompts to copy it up (interactive — tf exec inherits stdio)
> wf vm terraform exec "$PWD/infra/oracle-cloud" -- init -migrate-state
> wf vm terraform exec "$PWD/infra/oracle-cloud" -- plan   # MUST report no changes before you trust it
> ```
>
> If no machine has usable local state any more, adopt the live resources
> instead with `terraform import` (see the section above) — importing every
> resource in `main.tf` — and only then treat the remote state as authoritative.

**No state locking.** OCI's S3 compat provides neither DynamoDB nor the
conditional-write primitive `use_lockfile` needs, so concurrent applies can
corrupt state. Coordinate by hand; versioning makes a corruption recoverable.

## Teardown

```bash
wf vm terraform exec "$PWD/infra/oracle-cloud" -- destroy
```

`terraform.tfvars` can hold the Bitwarden token and is gitignored — keep it off
version control. Local `*.tfstate` files stay gitignored too, but they are no
longer the source of truth once the migration above has been done.
