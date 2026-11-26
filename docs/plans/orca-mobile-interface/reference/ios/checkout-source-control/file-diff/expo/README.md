# Expo file review on the rca iPhone

The [published iOS build](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/8160c066-072d-4f90-9830-59b1fea0ad26) opened changed files from the normal checkout menu on the paired `rca` iPhone 18 Pro Max. The selected checkout was `native-fixture`, detached at `f5b0c44`.

In **Changes**, opening `e2e-tests.yml` showed the first of 1,497 files with zero viewed. Expanding the 352 unchanged lines revealed the beginning of the workflow. **Viewed · next file** opened `pipeline.yml` and increased the count to one. Previous returned to `e2e-tests.yml` with its viewed state. [Branch file](branch-file.png) · [Expanded lines](expanded.png) · [Next file](viewed-next.png) · [Recording](branch.mp4)

In **Commits**, opening the first file in **Pack offline Tamil lesson audio** showed `import_tamil.py`, **1 of 8 in this commit**, and **+17 −4**. Next file opened `anki-to-curated-lessons.md`. Previous returned to the first diff. [Commit file](commit-file.png) · [Next file](commit-next.png) · [Recording](drill.mp4)

## Comparison with installed Orca

Exact RGBA comparison covers the full 1,320-pixel width from physical row 165 through 2,759, inclusive, in both captures from the same simulator. The status bar and home indicator are outside that region. [Measurements](measurements.json) · [Installed Orca](../README.md)

| Screen | First Expo capture, differing pixels | Published capture, differing pixels | Pixels compared |
| --- | ---: | ---: | ---: |
| branch-file | 252,450 | 163,738 | 3,425,400 |
| commit-file | 1,213,636 | 732,980 | 3,425,400 |

The T3 endpoint `GET /api/mobile/source-control/file` takes the selected checkout, immutable head and optional base, file path, and optional old path for renames. The commit comparison returned the exact 8,255 parent bytes and 8,726 committed bytes for `import_tamil.py`. As a branch change the original was absent at the merge base. Both responses were gzip-compressed. [Paired gateway measurements](gateway.json)

## Reproduce

Open the selected checkout's **Source Control** screen in Expo Go. After the first branch file appears, run [branch.lua](branch.lua) with `wf ios test` and `--env out=<absolute-output-directory>`. For the commit recording, open **104 commits → Pack offline Tamil lesson audio** and run [drill.lua](drill.lua) from that commit's detail screen. Use a fresh `--test-output-dir` for each recording.

Viewed marks are held while this Source Control screen is open. Cross-launch review state, review notes, binary previews and the overflow menu remain open.

The exported web app also passed the branch and commit journeys. [Browser runbook](browser.lua) · [Browser recording](browser.mp4)
