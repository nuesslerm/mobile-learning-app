# A slim shared Android device and a VM-run test process

The approved plan is `/root/.claude/plans/declarative-knitting-hummingbird.md` (copied into this directory's progress ledger as ticked steps). Short form:

- A. The shared redroid: recreate it from a fresh volume, make rollback real (stop the container, clear state, start), find the packages that break boot after a restart, and make a slim set that provably survives a restart.
- B. The VM test process: a device lease, a queue and a worker on the VM (Lua scripts plus the local opencode agent), runbook front matter that assigns scripts to packages, a GitHub workflow on the vm runner that enqueues the changed tests per push to dev after the Metro deploy, the second-display experiment, and flow step 4 (Expo Go on the device against the running dev Metro).
- C. Benchmarks: an Android suite with a real cold-start marker and baselines on the slim device; trustworthy web suites with re-recorded baselines.
- D. Gates G1 to G3 on web and Android as queued jobs, a fresh EAS Update, docs and the report, everything pushed.
