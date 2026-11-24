---
title: "The Android e2e shard fails when the speech-to-text sidecar cannot download its model (Hugging Face 503)"
status: open
area: ci
paths:
  - .github/workflows
  - clis/pipeline/src/test_plan.rs
opened: 2026-11-23
last_verified: 2026-11-23
links:
  - docs/gaps/android-stubbed-run-local-goldens-stale.md
---

# The Android e2e shard fails on an STT model download

On dev head 2ca7deb7 the mobile_build run's `android-e2e-tests (2/2)` job failed in its step "The backends on this runner": `wf test backends up --only stt,opencode-server,rca-service,leetcode-backend` ended with `stt ended with exit status: 1 before /health answered`; the sidecar's log shows `huggingface_hub` failing with `HTTP status server error (503 Service Unavailable)` from the Hugging Face CDN while downloading the model. No e2e unit ran or failed. The previous head (2215ea38) passed the same job, so this is a transient external failure, not a change of ours. The sidecar is only needed by the paused voice-edit and review-sheet-keyboard units (decision 104); a later cleanup can drop `stt` from the shard's `--only` list while those stay unfocused, or cache the model. No agent session can re-run a failed job.
