# Cold-run parity: self-hosted vs GitHub-hosted

The check before the self-hosted flavor is removed: one commit pushed with `[services:all] [cache:no]` runs every test stage and every deploy leg on both flavors of `pipeline.yml` with no Nx replay and no layer cache, whatever files the commit changed, so each job runs for real. It costs about 190 hosted minutes, so it is for a deliberate comparison only. The flavors are identical when every job has the same conclusion and the same test counts, and the artifacts they publish (images, the wf release, the run bundle) have the same shape. Durations differ by design and are not a parity criterion.

Compare with:

```sh
gh run view <run> --json jobs --jq '.jobs[]|"\(.conclusion)\t\(.name)\t\((.completedAt|fromdate)-(.startedAt|fromdate))s"'
```

Results are recorded below, newest first.
