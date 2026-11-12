# Cold-run parity: self-hosted vs GitHub-hosted

The check before the self-hosted flavor is removed: one commit pushed with `[cache:no] [ci:android] [services:vm-ops-api]` runs both flavors of `pipeline.yml` with no Nx replay, the Android job on and a real image build, so each job runs for real. The flavors are identical when every job has the same conclusion and the same test counts, and the artifacts they publish (images, the wf release, the run bundle) have the same shape. Durations differ by design and are not a parity criterion.

Compare with:

```sh
gh run view <run> --json jobs --jq '.jobs[]|"\(.conclusion)\t\(.name)\t\((.completedAt|fromdate)-(.startedAt|fromdate))s"'
```

Results are recorded below, newest first.
