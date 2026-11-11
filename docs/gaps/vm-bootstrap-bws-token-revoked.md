---
status: blocked
area: vm-infra
paths:
  - "infra/oracle-cloud/main.tf"
  - "infra/oracle-cloud/cloud-init.sh.tftpl"
  - "infra/oracle-cloud/terraform.tfvars*"
  - "infra/oracle-cloud/agent-shell/**"
opened: 2026-08-21
last_verified: 2026-10-04
links:
  - solutions/architecture/provisioning-required-secrets-are-a-blast-radius.md
  - solutions/security/verify-bitwarden-credential-with-doctor-before-claiming-unavailable.md
---
# The VM's bootstrap Bitwarden read token is revoked; a VM recreate with a working token is owner-gated

## What is broken

Every app's bootstrap read token on the VM host answers `[400 Bad Request] {"error":"invalid_client"}`,
vm-ops-api's and agent-shell's included. Every redeploy (the `dev` trunk deploy too) falls back to the
stale `latest` image, so a VM job gets no `GITHUB_TOKEN` or provider keys. This also parks
`docs/archive/plans/vm-puppeteer` units e1/e2 (`GET /services/agent-shell/vm-jobs` is 404 on the running
server; e1's script is stashed as "vm-puppeteer e1 wip").
History: `BWS_ADMIN_ACCESS_TOKEN` leaked to OCI console history via `set -x` in
`cloud-init.sh.tftpl` (fixed, console objects deleted, 2026-08-21) and had to be rotated.

## Why blocked

The token is the credential that opens the vault, so the VM cannot fetch it from the vault: it reaches
the host only through `bws_read_access_token` in tfvars at instance creation. The recreate is
authorized (`infra/oracle-cloud/CLAUDE.md`), but the auto-mode classifier denies the replace step
(`wf vm terraform exec ... -replace=oci_core_instance.main`) and an agent must not grant itself a refused permission.
Minting a machine-account token is web-vault-only (`bws` has no token subcommand).

## What unblocks it

The owner frees the VM's tailnet names with `wf vm net device retire --force --apply --name
<app>-main` (one `--name` per app in `local.vm_apps`), then runs
`wf vm terraform exec infra/oracle-cloud -- apply -replace=oci_core_instance.main` with
`TF_VAR_bws_read_access_token` set to a working read token (or adds an allow rule for those two
commands). Then rotate agent-shell's machine token (`wf secret set ...`, redeploy agent-shell) and run
`wf offload plan vm-puppeteer`.
