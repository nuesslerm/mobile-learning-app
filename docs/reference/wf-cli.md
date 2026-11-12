# The `wf` CLI

Every `wf` command with its options and real examples, generated from the help texts (`clis/wf*/src/help/*.txt`, the same text `wf <command> --help` prints) at dev `96788b01e`. Examples come from how the repo calls each command (CI workflows, e2e scripts, goals, runbooks); where the repo has none, one is written from the usage line.

## The tree

```
android                      The Android harness: the one redroid on the VM, driven over adb and its on-device UI
  adb                        Runs adb with the device's serial in front (ANDROID_SERIAL, else WF_ANDROID_SERIAL; plai
  hierarchy                  The screen's elements as JSON (Maestro's `maestro hierarchy`), so an agent can write a
  lease                      One holder at a time on the device, a FIFO line behind it, and a TTL so a crashed
  list-devices               The device the container --name (default WF_ANDROID_NAME, else rdl) runs, read on th
  stack                      Maps a JavaScript stack the app logged to source lines through an EAS Update's source maps (stdin or --file)
  test                       Runs one Lua script against the Android device (redroid; adb by --device, ANDROID_SERI
  start-device               Starts the on-demand redroid on the VM (container --name, default WF_ANDROID_NAME, else
  stop-device                Stops the on-demand redroid gracefully (Android gets 30 s to write its state) and remo
browser                      The browser harness: Chromium framed as a phone, driven through the browser daem
  check-taps                 A post-run check that every tap in a recording's input log (default <video>.inpu
  debug-eval                 Evaluates an expression in the paused top frame; a throw fails (exit 1). Browser
  debug-pause                Holds the page at the next statement it runs. While paused the page runs nothing
  debug-resume               Releases the paused page, or cancels a pending pause. Browser only, as set-break
  debug-step                 One step from the pause (default over). Browser only, as set-breakpoint. Result:
  hierarchy                  The screen's elements as JSON (Maestro's `maestro hierarchy`), so an agent can w
  list-devices               What a verb can drive, without starting anything: the daemon, its open sessions 
  test                       Runs one Lua script against the browser harness (Chromium framed as the phone pr
  set-breakpoint             Arms a pausing breakpoint on an original source line, resolved through the page'
  start                      Starts the browser daemon for this checkout, or reuses the running one. Frozen f
  start-device               Opens browser session N, framed as the phone preset P (default WF_BROWSER_SCREEN
  status                     The running daemon's URL and sessions, or exit 1 when none runs. Frozen as start
  stop                       Stops the daemon and every session in it, or exits 1 when none runs. Frozen as s
  stop-device                Closes the session (--device, else the only open one), or every session with --a
  tailnet-forward            Forwards one tailnet host:port to a local port through the daemon's tailnet acce
  tunnel                     Publishes the deployed VM services (vm-ops-api, remote-chat-agent, leetcode-back
cache                        The build cache: one OCI Object Storage bucket for every cache layer
  nx                         Run a command with Nx served from the bucket
  sccache                    Run a command with Rust compiles served from the bucket
  gc                         Delete cache objects: --evict-to <bytes>, --prefix <p>, or --status
  purge                      Start a new empty generation (every kind starts over; the old entries expire by lifecycle)
  restore                    Fill an empty local pnpm store (pnpm) or app build directory (metro) from the bucket
  save                       Upload the pnpm store after a green install, or the app build directory after a build
  status                     Bytes stored and requests this month against the caps; exits 1 above either
check                        Verify a change or the repository
ci                           What a CI run says about itself: `ci comment` posts what failed on the pushed commit
  comment                    Comment on the pushed commit what failed in this CI run, so whoever watches the commit
exec                         The executor of an rca workspace: attaches a directory to the rca service and performs a model's actions there (the separate binary rca-exec)
  serve                      Attach to rca and serve a workspace until the channel closes
flow                         The flow harness: vitest over the app's *.flow.test.tsx (harness doubles, jsdom)
  goldens                    The flow harness's golden pass, with no device (`wf flow run` calls it after vite
  run                        Runs the flow tests of frontend/<app> (vitest, vitest.flow.config.ts, --configLo
log                          Emit structured log lines to BetterStack (`wf log emit`, also shipped as the sta
  emit                       Emit one structured log line to BetterStack; prints {"sent":...} and always exit
agent-config                 The .agents/ contract rendered into every tool's config
  sync                       Render the .agents/ contract into every tool config
bench                        App benchmarks and CLI measurements
  agent-proof                Agent proof: an agent gets a fixed four-step task (open <base>/leetcode/two-sum,
  cli                        Measure CLI cold start, wall time and peak RSS per command (python3 rusage, fres
knowledge                    docs/solutions knowledge index (rank, index, stale) and the Gemini Gem context bundle (index --gemini)
  index                      Rebuild the docs/solutions index; with --gemini, build one ARCHITECTURE.md + repomix bundle and upload it over one Drive file for the Gemini Gem
  rank                       Rank docs/solutions learnings relevant to these files
  stale                      List learnings not touched in --days
offload                      Work on the cheap-model ladder: ask, stats, plan, worker, ladder
  ask                        Run one prompt on the cheap-model ladder (rca): an argument, --prompt, --prompt-
  plan                       Run docs/plans/<slug>/units.jsonl on git-leased worktrees (the git-state worker)
  stats                      The run-log summary of the cheap-model ladder
  ladder                     The cheap-model ladder's administration
    clear-cooldown           Drop a pool from the cooldown list so the ladder tries it again
    sync-pricing             Refresh model-pricing.json from the OpenRouter model list
  api                        A script is plain Lua 5.4 with these globals
parity                       How far the agent API mappers are from parity with the pinned phone apps
  coverage                   Scan the pinned phone app and build its coverage table (--check compares)
secret                       Manage secrets (macOS Keychain locally; Bitwarden in production — pick with --ba
  cache                      Remove the local copy of the vault
  create                     Create a vault secret, using its registry description as the note
  delete                     Remove secret — macOS backend only
  doctor                     Compare the registry against the vault in BOTH directions and exit non-zero on d
  env                        Print KEY=value lines for all available secrets (eval-safe)
  get                        Print raw value (exits 1 if missing)
  list                       Show all registered secrets and their sources
  login                      Capture interactive tool logins (claude, codex, ...): `wf secret login <tool>` r
    capture                  Run an ARBITRARY interactive login on the shared VNC desktop from a JSON spec: o
    done                     Signal that an interactive login is finished
    launch-url               Navigate the shared VNC desktop's browser to <url> (e.g. a magic-link URL copied
    list                     List tools with a known auth descriptor
    listener                 Run the auth-session listener (inside the shared desktop container): mints the t
  move                       Move a vault secret to another project in place, keeping its key, value, note an
  push-notes                 Sync each secret's registry description into its Bitwarden note (one-way: regist
  registry                   Secrets registry maintenance
    sync                     Rebuild wf after editing registry.json (the registry is compiled into it), then 
  rename                     Rename a vault secret in place, preserving its id (so UUID-addressed consumers k
  set                        Store secret (reads from stdin if value omitted) — macOS backend only
  sync-env                   Resolve keys (or --for <app>) through the active backend and write a .env — the 
  update                     Replace an EXISTING vault secret's value in place, keeping its key, note, projec
ship                         Build a service and the sidecar images its compose pulls on the same tag on the 
  eas                        Publish the app to Expo as an EAS Update (frontend/mla-tamagui's publish:eas script)
  retire                     Take down the compose project of a service that was removed from the service lis
t3                           The T3-compatible server (T3 Code protocol 2) and its pairing codes (the separate binary t3-serve)
  serve                      Run the server until it is stopped (default 127.0.0.1:3774)
  pair                       Mint a one-time pairing code and print its pairing URL as JSON
test                         The e2e tests: scripted Lua tests in e2e/<name>.lua, each run through its harnes
vm                           The VM: run a script in agent-shell, read logs, recover, and its sub-areas
  creds                      OCI credentials for Terraform
    sync                     Pull the 6 OCI_* secrets from the vault and write ~/.oci/oci_api_key.pem + the m
  deps                       Declarative dependency manifest for the shared agent workspace, replayed at cont
    add                      Record a dependency in the manifest
    install                  Install a dependency inside agent-shell now AND record it in the manifest so it 
    list                     List every entry currently in the manifest
    remove                   Remove a dependency from the manifest
    replay                   Install every manifest entry not already present
  exec                       Run a local script inside agent-shell on the VM; by default streams output until
  ledger                     Read one of the VM ledger files (allowlist: opencode-dispatch-runs.jsonl, pool-c
    cat                      Print a VM ledger file verbatim
  logs                       Recent BetterStack log lines (default: last 10m of the backend source), optional
  net                        Tailnet device + auth-key management
    auth-key                 Rotate TS_VM_AUTH_KEY before Tailscale’s 90-day cap expires it
      status                 Report how long TS_VM_AUTH_KEY has left; exits non-zero when rotation is due (re
    device                   Tailnet device records
      list                   Show every tailnet device with its staleness verdict, flagging collision-suffixe
      retire                 Delete stale device records so the next node can claim the bare hostname vm-gate
    resolve                  The name each tailnet host answers to now: the base name, else the first of <nam
    up                       Join this machine to the tailnet (starts tailscaled if stopped)
  ops                        Operate vm-ops-api itself (health, self-redeploy) -- not one of the apps it mana
    drill                    Second-door drill: take the vm-ops-api container down on purpose and measure wh
    health                   GET /health -- confirms vm-ops-api is reachable and which image/commit it is run
    logs                     docker logs for one of vm-ops-api's managed containers -- the only way to see wh
    pair                     Mint an orca:// pairing link in the deployed orca-serve container and print it (
    probe                    GET /health of each app on the tailnet at the name its host answers to now; exit
    redeploy-self            Have vm-ops-api build its OWN image at the dev head and swap itself in, fire-and-
    stats                    GET /stats -- what holds the VM's memory and CPU
  recover                    Health-sweep every app via vm-ops-api and redeploy unhealthy ones with recreate=
  terraform                  Terraform for infra/oracle-cloud
    apply                    Guarded apply: free-tier check on every planned create/resize, post-apply conver
    exec                     Run terraform with the remote-state credentials injected (plan, output, state li
```

## Gone, and what replaces them

| Was | Now |
|---|---|
| `wf test device` | wf test run |
| `wf test metrics` | wf test publish (it posts the row) |
| `wf test record` | nothing: bundles are the record |
| `wf test enqueue / queue verbs` | the e2e-tests job in CI, or wf test run |
| `wf test plan --latest` | `wf test results --query "<terms>"` |
| the top-level `ask`, `plan` and `worker` | wf offload ask, wf offload plan, wf offload worker |
| the old `worker visual` | wf check --visual |
| the `repo` group's `bench` verbs | wf bench ... |
| the `repo` group's `knowledge` verbs | wf knowledge ... |
| the `repo` group's `sync [--check]`, and the top-level `mcp-guard` | wf agent-config sync [--check], wf agent-config mcp-guard |
| the `repo` group's `ladder` verbs | wf offload ladder ... |
| the `repo` group's `bundle [--check]` | pnpm --filter mla-tamagui run build:editor-bundle (check:editor-bundle) |
| `wf run <script>` | wf android test <lua> / wf browser test <lua> |
| android run, browser run (renamed; each an unknown verb since H8) | wf android test <lua>, wf browser test <lua> (`--env`, `--test-output-dir`, `--logpoint` on the browser only; `--goldens` is gone) |
| browser tap, type, press, navigate, trace, screenshot, snapshot, audio-inject, video-check, open, close, list, debug-set (renamed; each an unknown verb since H8) | wf browser tap-on, input-text, press-key, open-link, draw-strokes-on, take-screenshot (or assert-screenshot), hierarchy, set-microphone-input, check-taps, start-device, stop-device, list-devices, set-breakpoint |
| browser goldens (renamed; an unknown verb since H8) | wf flow goldens |
| android tap, type, press, open, trace, screenshot, keyboard, edit-text, warnings, record start/stop, adb shell, up, down, features (renamed; each an unknown verb since H8, and adb shell is raw adb again) | wf android tap-on, input-text, press-key (or back), open-link, draw-strokes-on, take-screenshot, get-device-state keyboard, get-element --id CodeEditorInput, get-logs (and clear-logs), start-recording/stop-recording, run-shell, start-device, stop-device, list-devices |
| android text, find, wait, stroke, launch, install, help, start, stop, status (dropped; each exits 2 saying why) | input-text, get-element and hierarchy, extended-wait-until, draw-strokes-on, open-link, adb install -r, --help, start-device, stop-device, list-devices |
| `--serial`, `--identifier` (android); `lease --device`; `WF_ANDROID_DEVICE`; `start-device --print` (renamed; each an unknown option since H8, and WF_ANDROID_DEVICE is ignored) | `--device`; `lease --name`; `WF_ANDROID_NAME`; `--dry-run` |
| `--name` (browser); `WF_BROWSER_DEVICE` (renamed; `--name` an unknown option since H8, and WF_BROWSER_DEVICE is ignored) | `--device`; `WF_BROWSER_SCREEN_SIZE` (or `--screen-size`) |
| browser eval, outer-eval, shell-eval, wait-observe, network-stub, network-stub-hits, debug-hits, debug-clear, network-proxy, cookies, memory, coverage, check-clipping, upload, opened-urls, preflight, camera-inject, clipboard, drag, scroll, permission, set-device, stroke (dropped; each exits 2 saying why) | typed reads (get-element, hierarchy, get-logs), Lua run state (device:add_network_stub, test --logpoint), or the daemon RPC the clients call |

## Global options

| Option | Meaning |
|---|---|
| `-V, --version` | output the version number |
| `--json` | output machine-readable JSON lines |
| `--frontend <name>` | frontend app to target (mla-tamagui) (default: "mla-tamagui") |

## android

### `wf android`

The Android harness: the one redroid on the VM, driven over adb and its on-device UI server. The emulator runs only in CI's android-e2e-tests job. A device verb drives the adb serial --device names, else ANDROID_SERIAL or WF_ANDROID_SERIAL, else the only device adb lists; the lifecycle and lease verbs name the container with --name (default WF_ANDROID_NAME, else rdl). Every verb prints one JSON line: {ok, command, platform, device, ms, result} with exit 0; a failure {ok:false, ..., error, next} with exit 1; a usage error {ok:false, error, usage} with exit 2; the lease held by someone else, exit 3. --json is accepted and ignored. `wf android <command> --help` prints one in full.

Usage: `wf android <command> [options]`

Examples:

```sh
wf android start-device --expo-go
wf android list-devices
wf android lease status
```

<details><summary>Full help</summary>

```text
Usage: wf android <command> [options]

The Android harness: the one redroid on the VM, driven over adb and its on-device UI server. The emulator runs only in
CI's android-e2e-tests job. A device verb drives the adb serial --device names, else ANDROID_SERIAL or WF_ANDROID_SERIAL,
else the only device adb lists; the lifecycle and lease verbs name the container with --name (default WF_ANDROID_NAME,
else rdl). Every verb prints one JSON line: {ok, command, platform, device, ms, result} with exit 0; a failure {ok:false,
..., error, next} with exit 1; a usage error {ok:false, error, usage} with exit 2; the lease held by someone else, exit 3.
--json is accepted and ignored. `wf android <command> --help` prints one in full.

Devices:
  start-device [--name N] [--screen WxH@dpi] [--port P] [--with F,...] [--slim | --stock] [--expo-go] [--dry-run]
                         boots the redroid on the VM (slim by default) and waits for it
  stop-device [--name N] [--dry-run]
                         removes it, keeping its /data volume (idle cost zero)
  list-devices [--name N]
                         the device: booted, the slim set proved, each feature on or off, its lease
  hierarchy [--device D] the screen's elements as JSON, to write a selector from
  stack <group> [--platform P] [--file F]
                         maps a logged JavaScript stack (stdin or --file) to source lines through an EAS Update's source maps
  lease [acquire|heartbeat|release|status] [--name N] [--wait s] [--ttl s] [--who W] [--id ID] [--local]
                         one holder at a time on the device (FIFO, TTL)
  adb [adb-args...]      raw adb with the serial in front (lease-guarded; output not wrapped)

Tests:
  test <script.lua>      runs one Lua test (`wf android test --help`); `test --api` prints the script API

A dropped verb (text, find, wait, stroke, launch, install, help, start, stop, status) exits 2 with {error:"dropped",
why, use}; any other unknown verb exits 2 with {error:"unknown verb '<verb>'", verbs, usage}.
```

</details>

### `wf android adb`

Runs adb with the device's serial in front (ANDROID_SERIAL, else WF_ANDROID_SERIAL; plain adb when neither is set). Everything after `adb` goes to adb verbatim, and adb's own output and exit code come back unwrapped. Lease-guarded: it exits 3 naming the holder while someone else holds the device. `wf android run-shell` runs a shell command and answers in the envelope.

Usage: `wf android adb [adb-args...]`

Examples:

```sh
wf android adb devices
wf android adb install -r app.apk
```

<details><summary>Full help</summary>

```text
Usage: wf android adb [adb-args...]

Runs adb with the device's serial in front (ANDROID_SERIAL, else WF_ANDROID_SERIAL; plain adb when neither is set).
Everything after `adb` goes to adb verbatim, and adb's own output and exit code come back unwrapped. Lease-guarded: it
exits 3 naming the holder while someone else holds the device. `wf android run-shell` runs a shell command and answers
in the envelope.

Options:
  -h, --help  display help for command
```

</details>

### `wf android hierarchy`

The screen's elements as JSON (Maestro's `maestro hierarchy`), so an agent can write a selector: every on-screen node with a resource id, an accessibility label, its own text or an EditText's value, in tree order, at most 500. Bounds are the on-screen part. Read from the on-device UI server, or a uiautomator dump with WF_ANDROID_UIA=dump. Every failed wait writes the same dump as evidence. --device is the adb serial (default ANDROID_SERIAL, WF_ANDROID_SERIAL, else the only device).

Usage: `wf android hierarchy [--device D]`

Examples:

```sh
wf android hierarchy --device 127.0.0.1:5555
```

<details><summary>Full help</summary>

```text
Usage: wf android hierarchy [--device D]

The screen's elements as JSON (Maestro's `maestro hierarchy`), so an agent can write a selector: every on-screen node
with a resource id, an accessibility label, its own text or an EditText's value, in tree order, at most 500. Bounds are
the on-screen part. Read from the on-device UI server, or a uiautomator dump with WF_ANDROID_UIA=dump. Every failed wait
writes the same dump as evidence. --device is the adb serial (default ANDROID_SERIAL, WF_ANDROID_SERIAL, else the only
device).

Result: {platform, elements: [{id, label, text, class, bounds: {left, top, right, bottom}, frame}]}
```

</details>

### `wf android lease`

One holder at a time on the device, a FIFO line behind it, and a TTL so a crashed holder frees it. The state lives on the VM; a sandbox reaches it through `wf vm exec`. The verbs that change the device (tap-on, swipe, scroll-until-visible, input-text, press-key, back, open-link, draw-strokes-on, set-microphone-input, run-shell, start-recording, stop-recording, clear-logs and adb) exit 3, naming the holder, while someone else holds the lease (checked where the state is local, on the VM). Reads, start-device and stop-device do not need it.

Usage: `wf android lease [options] [acquire|heartbeat|release|status]`

| Option | Meaning |
|---|---|
| `--wait <seconds>` | how long to wait in line (default 600) |
| `--no-wait` | fail at once when the device is busy |
| `--ttl <seconds>` | lease length without a heartbeat (default 600) |
| `--who <name>` | holder name (default: WF_LEASE_WHO or host:pid) |
| `--id <leaseId>` | the lease to heartbeat or release |
| `--clean-package <pkg>` | with release: also `pm clear` this app |
| `--name <container>` | container (default: WF_ANDROID_NAME, else rdl; was --device) |
| `--local` | run on this machine instead of through the VM |

Examples:

```sh
wf android lease --wait 600 --who sandbox-a
wf android lease status
wf android lease release --id <leaseId> --clean-package host.exp.exponent
```

<details><summary>Full help</summary>

```text
Usage: wf android lease [options] [acquire|heartbeat|release|status]

One holder at a time on the device, a FIFO line behind it, and a TTL so a crashed
holder frees it. The state lives on the VM; a sandbox reaches it through
`wf vm exec`. The verbs that change the device (tap-on, swipe, scroll-until-visible,
input-text, press-key, back, open-link, draw-strokes-on, set-microphone-input,
run-shell, start-recording, stop-recording, clear-logs and adb) exit 3, naming the
holder, while someone else holds the lease (checked where the state is local, on
the VM). Reads, start-device and stop-device do not need it.

Actions:
  acquire (default)      take the device or wait in line; exit 3 when it is busy
  heartbeat --id <id>    keep the lease past its TTL
  release --id <id>      give it up (stops user apps, clears logcat); a waiting id leaves the line
  status                 who holds it and who is waiting

Result: acquire {granted: true, leaseId, who, ttlSeconds}; heartbeat and release
{action, leaseId}; status {holder: {id, who, heldMs, expiresInMs} | null, queue: [...]}.
Busy: {ok:false, error: "device busy: held by <who> (position <n>)", next}, exit 3.

Options:
  --wait <seconds>       how long to wait in line (default 600)
  --no-wait              fail at once when the device is busy
  --ttl <seconds>        lease length without a heartbeat (default 600)
  --who <name>           holder name (default: WF_LEASE_WHO or host:pid)
  --id <leaseId>         the lease to heartbeat or release
  --clean-package <pkg>  with release: also `pm clear` this app
  --name <container>     container (default: WF_ANDROID_NAME, else rdl; was --device)
  --local                run on this machine instead of through the VM
  -h, --help             display help for command

Environment: WF_ANDROID_LEASE_ID (the lease a verb runs under), WF_ANDROID_LEASE=none
(skip the check).
```

</details>

### `wf android list-devices`

The device the container --name (default WF_ANDROID_NAME, else rdl) runs, read on the VM: whether it finished booting, whether the slim set was proved to boot after a restart, and each feature of the catalog (clis/harness-android/src/android_features.json) as on, off or partial. Every package of the stock image is core (always kept), part of a named feature (on only with `wf android start-device --with <feature>`) or removed. An empty list when no such container runs.

Usage: `wf android list-devices [--name N]`

Examples:

```sh
wf android list-devices | jq '.result.devices[0]'
```

<details><summary>Full help</summary>

```text
Usage: wf android list-devices [--name N]

The device the container --name (default WF_ANDROID_NAME, else rdl) runs, read on the VM: whether it finished booting,
whether the slim set was proved to boot after a restart, and each feature of the catalog (clis/harness-android/src/android_features.json)
as on, off or partial. Every package of the stock image is core (always kept), part of a named feature (on only with
`wf android start-device --with <feature>`) or removed. An empty list when no such container runs.

Result: {devices: [{name, serial, boot: {completed, activityService}, restartProven, disabledTotal, core: {state, packages},
removed: {state, packages}, features: [{feature, state: on|off|partial, what, packages}], lease: {holder, queue} | null}]}
```

</details>

### `wf android test`

Runs one Lua script against the Android device (Maestro's `maestro test`; redroid, adb by --device, ANDROID_SERIAL or WF_ANDROID_SERIAL). The run starts from the Expo Go already running (appStart "reused"); --cold-start force-stops it first, so the script's first deep link loads the app from scratch (appStart "cold"; `wf test run` does this once per job, after a failed unit and when the target changes: another update group). The report's timing carries appStart and startupS. The script drives `device` with the command table's methods (`wf android test --api`); --env K=V fills its `env` table. assert_screenshot compares with its golden (docs/runbooks/goldens/android/, or --goldens-dir) on every run; --update-goldens writes them instead. The test renews its device lease every 60 s while it runs.

Every run ends with <test-output-dir>/report.{json,html} (default .agents/tmp/runs/<script>-<ms>; schema in `wf android test --api`), timing.jsonl and the recording; --publish uploads the run as a bundle (`wf test publish`) and posts its test_runs row. Output: the script's log lines and its report:, bundle:, recording: and goldens: lines.

Usage: `wf android test <script.lua> [--env K=V]... [--publish] [--test-output-dir D] [--no-record] [--update-goldens] [--goldens-dir D] [--device D] [--cold-start]`

Examples:

```sh
wf android test --api
wf android test e2e/editor-typing.lua --env base=exp://u.expo.dev/<projectId>/group/<group>/-- --publish
```

<details><summary>Full help</summary>

```text
Usage: wf android test <script.lua> [--env K=V]... [--publish] [--test-output-dir D] [--no-record] [--update-goldens]
                       [--goldens-dir D] [--device D] [--cold-start] [--speed N]
       wf android test --api [--json]

Runs one Lua script against the Android device (Maestro's `maestro test`; redroid, adb by --device, ANDROID_SERIAL
or WF_ANDROID_SERIAL). The run starts from the Expo Go already running (appStart "reused"); --cold-start force-stops
it first, so the script's first deep link loads the app from scratch (appStart "cold"; `wf test run` does this once
per job, after a failed unit and when the target changes: another update group). The report's timing carries appStart and startupS. The script
drives `device` with the command table's methods (`wf android test --api`); --env K=V fills its `env` table.
assert_screenshot compares with its golden
(docs/runbooks/goldens/android/, or --goldens-dir) on every run; --update-goldens writes them instead. The test
renews its device lease every 60 s while it runs. --speed N (else WF_TEST_SPEED; 1 to 4, default 1, anything else is a
usage error) runs the user's gestures at N times their pace: the swipe verb's default 400 ms and scroll_until_visible's
600 ms drags are divided by N, never below 250 ms. A duration the script passes, a stroke, typed text (adb types it
at once), a sleep, a wait and a timeout keep their own time. The report's speed says which ran. CI runs Android at 1:
an Android swipe lifts the finger while it still moves, so a shorter one flings further, and a toolbar key it brings in
can fly past the screen; speed 2 waits for a published editor-typing run at 2 with no toolbar key typed on the keyboard.

Every run ends with <test-output-dir>/report.{json,html} (default .agents/tmp/runs/<script>-<ms>; schema in
`wf android test --api`), timing.jsonl and the recording; --publish uploads the run as a bundle (`wf test publish`)
and posts its test_runs row. Output: the script's log lines and its report:, bundle:, recording: and goldens: lines.

Logs instead of a debugger: the run id (<script>-<ms>) goes into every launch link as ?wfRun=<id>, the app puts it on each
BetterStack line it sends, and after the script the harness reads those lines (waiting up to WF_ANDROID_LOGS_WAIT_S, default
30 s, for ingest) and the report's App logs section lists them, crashes first, their stacks mapped to source lines through
the update group's maps (`wf android stack`). It says when lines may be missing (slow ingest, no ingest pair in the build).

Removed: --goldens (assert_screenshot always compares); --logpoint (an Android run loads the EAS Update, which has no inspector: it is
refused with exit 1, and there is no Android debugger; the browser run has one).

Exit: 0 pass, 1 the script failed or the harness errored (the report says which), 2 a usage error, 124 stopped by SIGTERM (report written).
```

</details>

### `wf android stack`

Maps a JavaScript stack the app logged (an `app.crash` line from BetterStack) to source lines. The stack is read from --file, else stdin. <group> is the EAS Update's group id (in its link, `exp://u.expo.dev/<project>/group/<group>`, and in the run summary); the source map comes from the verification bucket, where `wf ship eas` stores the group's maps and summary.json. Hermes frames (`at name (address at bundle:LINE:COL)`) map to `name (source:LINE:COL)`; a frame the map cannot place stays as logged and carries mapped: false. `wf android test` does the same for every crash and error line of its run.

Usage: `wf android stack <group> [--platform P] [--file F]`

Examples:

```sh
wf android stack 0d3a2f1e-9c64-4a7b-8d52-1f6c7e9ab001 --file crash.txt
```

<details><summary>Full help</summary>

```text
Usage: wf android stack <group> [--platform P] [--file F]

Maps a JavaScript stack the app logged (an `app.crash` line from BetterStack) to source lines. The stack is read from --file,
else stdin. <group> is the EAS Update's group id (in its link, `exp://u.expo.dev/<project>/group/<group>`, and in the run
summary); the source map comes from the verification bucket, where `wf ship eas` stores the group's maps and summary.json.
Hermes frames (`at name (address at bundle:LINE:COL)`) map to `name (source:LINE:COL)`; a frame the map cannot place stays as
logged and carries mapped: false. `wf android test` does the same for every crash and error line of its run.

Options:
  --platform <p>         the platform's map (default android)
  --file <path>          read the stack from a file instead of stdin

Result: {group, platform, map, frames: [{raw, mapped, source, line, column, name}], text}
```

</details>

### `wf android start-device`

Starts the on-demand redroid on the VM (container --name, default WF_ANDROID_NAME, else rdl; adb on port --port, default 5555, of the agent-shell node) and waits for it to boot. A fresh device boots slim: every package outside the core set and the --with features is disabled (`wf android list-devices` shows each feature). Idempotent: a running device at the same screen is kept as it is unless --slim, --with or --stock is given; one at another screen is stopped and booted again. Nothing stays resident between runs, so idle cost is zero. The script's progress goes to stderr; stdout carries the one JSON line. Not lease-guarded: nobody holds a lease on a device that is not running.

Usage: `wf android start-device [--name N] [--screen WxH@dpi] [--port P] [--with F,...] [--slim | --stock] [--expo-go] [--dry-run]`

| Option | Meaning |
|---|---|
| `--screen <WxH@dpi>` | screen (default: 480x1040@213: a Galaxy S25 in dp, 360 x 780, fewer pixels than 540x960@240) |
| `--expo-go` | install Expo Go 57.0.9 once booted (also on a device that is already running) |
| `--with <features>` | keep these features on, comma separated (camera,files,...) |
| `--slim` | apply the slim set to a device that is already running |
| `--stock` | enable every package again (no slimming) |
| `--name <container>` | container name (default: WF_ANDROID_NAME, else rdl), to run beside another device |
| `--port <n>` | host adb port (default: 5555) |
| `--dry-run` | print the VM script in result.script instead of running it (was --print) |

Examples:

```sh
wf android start-device --expo-go
wf android start-device --dry-run --with camera
wf android start-device --name rdl-bench --port 5556
```

<details><summary>Full help</summary>

```text
Usage: wf android start-device [--name N] [--screen WxH@dpi] [--port P] [--with F,...] [--slim | --stock] [--expo-go] [--dry-run]

Starts the on-demand redroid on the VM (container --name, default WF_ANDROID_NAME, else rdl; adb on port --port, default
5555, of the agent-shell node) and waits for it to boot. A fresh device boots slim: every package outside the core set and
the --with features is disabled (`wf android list-devices` shows each feature). Idempotent: a running device at the same
screen is kept as it is unless --slim, --with or --stock is given; one at another screen is stopped and booted again.
Nothing stays resident between runs, so idle cost is zero. The script's progress goes to stderr; stdout carries the one
JSON line. Not lease-guarded: nobody holds a lease on a device that is not running.

Options:
  --screen <WxH@dpi>     screen (default: 480x1040@213: a Galaxy S25 in dp, 360 x 780, fewer pixels than 540x960@240)
  --expo-go              install Expo Go 57.0.9 once booted (also on a device that is already running)
  --with <features>      keep these features on, comma separated (camera,files,...)
  --slim                 apply the slim set to a device that is already running
  --stock                enable every package again (no slimming)
  --name <container>     container name (default: WF_ANDROID_NAME, else rdl), to run beside another device
  --port <n>             host adb port (default: 5555)
  --dry-run              print the VM script in result.script instead of running it (was --print)

Result: {name, screen, port, expoGo}; with --dry-run {dryRun: true, script}
```

</details>

### `wf android stop-device`

Stops the on-demand redroid gracefully (Android gets 30 s to write its state) and removes the container (--name, default WF_ANDROID_NAME, else rdl), reporting the VM's MemAvailable before and after on stderr. The /data volume is kept, so the next boot stays fast. Not lease-guarded: a crashed run's stale lease must never keep CI's teardown from freeing the VM.

Usage: `wf android stop-device [--name N] [--dry-run]`

| Option | Meaning |
|---|---|
| `--name <container>` | container name (default: WF_ANDROID_NAME, else rdl) |
| `--dry-run` | print the VM script in result.script instead of running it (was --print) |

Examples:

```sh
wf android stop-device
wf android stop-device --dry-run
```

<details><summary>Full help</summary>

```text
Usage: wf android stop-device [--name N] [--dry-run]

Stops the on-demand redroid gracefully (Android gets 30 s to write its state) and removes the container (--name, default
WF_ANDROID_NAME, else rdl), reporting the VM's MemAvailable before and after on stderr. The /data volume is kept, so the
next boot stays fast. Not lease-guarded: a crashed run's stale lease must never keep CI's teardown from freeing the VM.

Options:
  --name <container>     container name (default: WF_ANDROID_NAME, else rdl)
  --dry-run              print the VM script in result.script instead of running it (was --print)

Result: {name, stopped: true}; with --dry-run {dryRun: true, script}
```

</details>

## browser

### `wf browser`

The browser harness: Chromium framed as a phone, driven through the browser daemon (`wf browser-daemon`). A session is a device: --device names it, and a verb drives the only open session when none is named (a running test's session only when --device names it). Every verb prints one JSON line: {ok, command, platform, device, ms, result} with exit 0; a failure {ok:false, ..., error, next} with exit 1; a usage error {ok:false, error, usage} with exit 2. --json is accepted and ignored. `wf browser <command> --help` prints one in full.

Usage: `wf browser <command> [options]`

Examples:

```sh
wf browser start-device --device primary --color-scheme dark
wf browser open-link http://127.0.0.1:8800/mla-tamagui/leetcode/two-sum
wf browser tap-on --id ProblemEditorRunLocalButton
```

<details><summary>Full help</summary>

```text
Usage: wf browser <command> [options]

The browser harness: Chromium framed as a phone, driven through the browser daemon (`wf browser-daemon`). A session is a
device: --device names it, and a verb drives the only open session when none is named (a running test's session only
when --device names it). Every verb prints one JSON line: {ok, command, platform, device, ms, result} with exit 0; a
failure {ok:false, ..., error, next} with exit 1; a usage error {ok:false, error, usage} with exit 2. --json is accepted
and ignored. `wf browser <command> --help` prints one in full.

Devices:
  start-device --device N [--screen-size P] [--frame F] [--color-scheme S] [--tailnet] [--plain] [--headless]
                         opens session N (starts the daemon when none runs)
  stop-device [--device N | --all]
                         closes the session, or every session
  list-devices           the daemon, the open sessions (with the pid of a test that owns one) and the screen presets
  hierarchy [--device N] the screen's elements as JSON, to write a selector from

Tests:
  test <script.lua>      runs one Lua test (`wf browser test --help`); `test --api` prints the script API

Checks and the tailnet:
  check-taps <video> [--inputs F]
                         every tap in a recording's input log shows its dot in the video (exit 1 when one does not)
  tailnet-forward --host H --port P [--local-port L]
                         forwards one tailnet host:port to a local port, for a page in the device frame
  tunnel [services...]   publishes the deployed VM services on localhost ports (foreground)

Debugger (browser only: there is no Android debugger):
  set-breakpoint <file:line[:condition]> [--device N]
  debug-pause [--timeout ms] | debug-step [over|into|out] | debug-eval <expr> | debug-resume   [--device N]

The daemon (frozen for @lib/browser-client and the cookie-capture service; human text, or one pino line with --json):
  start [--headless] [--screenshot-dir D] | status | stop

A dropped verb (eval, cookies, network-stub, ...) exits 2 with {error:"dropped", why, use}; any other unknown verb exits
2 with {error:"unknown verb '<verb>'", verbs, usage}.
```

</details>

### `wf browser test`

Runs one Lua script against the browser harness (Maestro's `maestro test`): Chromium framed as the phone preset --screen-size (else WF_BROWSER_SCREEN_SIZE, else iphone-15-pro; `wf browser list-devices` lists them), --frame screen|bezel|document (else WF_BROWSER_FRAME). The script drives `device` with the command table's methods (`wf browser test --api`); --env K=V fills its `env` table. --device names the session the test opens (default run-<pid>), marked with the test's pid, so a verb typed by hand never drives it. --logpoint arms a source-mapped logpoint before the first page loads; its hits land in <test-output-dir>/debug.jsonl. assert_screenshot compares with its golden (docs/runbooks/goldens/browser/, or --goldens-dir) on every run; --update-goldens writes them instead.

Every run ends with <test-output-dir>/report.{json,html} (default .agents/tmp/runs/<script>-<ms>; schema in `wf browser test --api`), timing.jsonl and, unless --no-record, run.mp4; --publish uploads the run as a bundle (`wf test publish`) and posts its test_runs row. Output: the script's log lines and its report:, bundle:, recording: and goldens: lines.

Usage: `wf browser test <script.lua> [--env K=V]... [--publish] [--test-output-dir D] [--no-record] [--update-goldens] [--goldens-dir D] [--logpoint file:line[:cond]]... [--device D] [--screen-size P] [--frame F]`

Examples:

```sh
wf browser test --api
wf browser test e2e/web-goldens.lua --env base=http://127.0.0.1:8080 --screen-size iphone-15-pro
```

<details><summary>Full help</summary>

```text
Usage: wf browser test <script.lua> [--env K=V]... [--publish] [--test-output-dir D] [--no-record] [--update-goldens]
                       [--goldens-dir D] [--logpoint file:line[:cond]]... [--device D] [--screen-size P] [--frame F]
       wf browser test --api [--json]

Runs one Lua script against the browser harness (Maestro's `maestro test`): Chromium framed as the phone preset
--screen-size (else WF_BROWSER_SCREEN_SIZE, else iphone-15-pro; `wf browser list-devices` lists them), --frame
screen|bezel|document (else WF_BROWSER_FRAME). The script drives `device` with the command table's methods
(`wf browser test --api`); --env K=V fills its `env` table. --device names the session the test opens (default
run-<pid>), marked with the test's pid, so a verb typed by hand never drives it. --logpoint arms a source-mapped
logpoint before the first page loads; its hits land in <test-output-dir>/debug.jsonl. assert_screenshot compares
with its golden (docs/runbooks/goldens/browser/, or --goldens-dir) on every run; --update-goldens writes them instead.

Every run ends with <test-output-dir>/report.{json,html} (default .agents/tmp/runs/<script>-<ms>; schema in
`wf browser test --api`), timing.jsonl and, unless --no-record, run.mp4; --publish uploads the run as a bundle
(`wf test publish`) and posts its test_runs row. Output: the script's log lines and its report:, bundle:,
recording: and goldens: lines.

Removed: --goldens (assert_screenshot always compares).

Exit: 0 pass, 1 the script failed or the harness errored (the report says which), 2 a usage error, 124 stopped by SIGTERM (report written).
```

</details>

### `wf browser start-device`

Opens browser session N, framed as the phone preset P (default WF_BROWSER_SCREEN_SIZE, else iphone-15-pro; `wf browser list-devices` lists the presets), and starts the daemon when none runs (not under WF_BROWSER_NO_START). --frame screen (default, or WF_BROWSER_FRAME) overlays the status bar and home indicator as iOS does; bezel draws the phone; document gives the chrome its own rows, for a page that is not the app. --tailnet (or WF_BROWSER_TAILNET) routes the page through the tailnet; --plain opens a plain page with no device frame. A preset given to --device exits 2 and names --screen-size. Record with start-recording and stop-recording.

Usage: `wf browser start-device --device N [--screen-size P] [--frame screen|bezel|document] [--color-scheme light|dark|system] [--tailnet] [--plain] [--headless]`

Examples:

```sh
wf browser start-device --device code --screen-size iphone-15-pro
wf browser start-device --device primary --color-scheme dark
```

<details><summary>Full help</summary>

```text
Usage: wf browser start-device --device N [--screen-size P] [--frame screen|bezel|document] [--color-scheme light|dark|system] [--tailnet] [--plain] [--headless]

Opens browser session N, framed as the phone preset P (default WF_BROWSER_SCREEN_SIZE, else iphone-15-pro;
`wf browser list-devices` lists the presets), and starts the daemon when none runs (not under WF_BROWSER_NO_START).
--frame screen (default, or WF_BROWSER_FRAME) overlays the status bar and home indicator as iOS does; bezel draws the
phone; document gives the chrome its own rows, for a page that is not the app. --tailnet (or WF_BROWSER_TAILNET) routes
the page through the tailnet; --plain opens a plain page with no device frame. A preset given to --device exits 2 and
names --screen-size. Record with start-recording and stop-recording.

Result: the session's summary {name, engine, renderMode, devicePreset, orientation, keyboardMode, colorScheme, ...}
```

</details>

### `wf browser stop-device`

Closes the session (--device, else the only open one), or every session with --all. The daemon keeps running: `wf browser stop` ends it.

Usage: `wf browser stop-device [--device N | --all]`

Examples:

```sh
wf browser stop-device --device code
wf browser stop-device --all
```

<details><summary>Full help</summary>

```text
Usage: wf browser stop-device [--device N | --all]

Closes the session (--device, else the only open one), or every session with --all. The daemon keeps running:
`wf browser stop` ends it.

Result: {closed: [names]}
```

</details>

### `wf browser list-devices`

What a verb can drive, without starting anything: the daemon, its open sessions (owner is the pid of the test that opened one; a verb drives that session only when --device names it) and the screen presets start-device takes.

Usage: `wf browser list-devices`

Examples:

```sh
wf browser list-devices
```

<details><summary>Full help</summary>

```text
Usage: wf browser list-devices

What a verb can drive, without starting anything: the daemon, its open sessions (owner is the pid of the test that opened
one; a verb drives that session only when --device names it) and the screen presets start-device takes.

Result: {daemon: {running, url}, devices: [{name, renderMode, devicePreset, orientation, colorScheme, owner, ...}],
screens: [{id, label, viewport: {width, height}, dpr, ...}]}
```

</details>

### `wf browser hierarchy`

The screen's elements as JSON (Maestro's `maestro hierarchy`), so an agent can write a selector: every on-screen element with a test id, an accessibility label, its own text or a field value, the app page first, then its same-origin frames (the editor's web view), at most 500. Bounds are clipped to the screen; text is a field's value for an input. Every failed wait writes the same dump as evidence.

Usage: `wf browser hierarchy [--device N]`

Examples:

```sh
wf browser hierarchy
wf browser hierarchy --device code
```

<details><summary>Full help</summary>

```text
Usage: wf browser hierarchy [--device N]

The screen's elements as JSON (Maestro's `maestro hierarchy`), so an agent can write a selector: every on-screen element
with a test id, an accessibility label, its own text or a field value, the app page first, then its same-origin frames
(the editor's web view), at most 500. Bounds are clipped to the screen; text is a field's value for an input. Every
failed wait writes the same dump as evidence.

Result: {platform, elements: [{id, label, text, bounds: {left, top, right, bottom}, frame}]}
```

</details>

### `wf browser check-taps`

A post-run check that every tap in a recording's input log (default <video>.inputs.jsonl, written beside a recording by start-recording or a test run) shows its tap dot in the video within 1500 ms. No daemon is needed.

Usage: `wf browser check-taps <video> [--inputs inputs.jsonl]`

Examples:

```sh
wf browser check-taps .agents/tmp/device-lab-g3/g3.mp4
```

<details><summary>Full help</summary>

```text
Usage: wf browser check-taps <video> [--inputs inputs.jsonl]

A post-run check that every tap in a recording's input log (default <video>.inputs.jsonl, written beside a recording by
start-recording or a test run) shows its tap dot in the video within 1500 ms. No daemon is needed.

Result: {taps, dotsFound, missing: [], pass: true}; a tap with no dot fails (exit 1) with the check as evidence and the
missing tap times in the error.
```

</details>

### `wf browser set-breakpoint`

Arms a pausing breakpoint on an original source line, resolved through the page's source maps (a path suffix is enough; an unknown file fails naming the files that have maps). The condition runs in the page's scope. When it hits, the page holds there: read it with debug-eval, move with debug-step, release it with debug-resume. A logpoint that never pauses is armed by the test (`wf browser test --logpoint file:line[:cond]`). Browser only: there is no Android debugger.

Usage: `wf browser set-breakpoint <file:line[:condition]> [--device N]`

Examples:

```sh
wf browser set-breakpoint src/loop.ts:3
wf browser set-breakpoint lib/logger.ts:95:level === "warn"
```

<details><summary>Full help</summary>

```text
Usage: wf browser set-breakpoint <file:line[:condition]> [--device N]

Arms a pausing breakpoint on an original source line, resolved through the page's source maps (a path suffix is enough;
an unknown file fails naming the files that have maps). The condition runs in the page's scope. When it hits, the page
holds there: read it with debug-eval, move with debug-step, release it with debug-resume. A logpoint that never pauses is
armed by the test (`wf browser test --logpoint file:line[:cond]`). Browser only: there is no Android debugger.

Result: {id, file, line, resolved, scripts}
```

</details>

### `wf browser debug-pause`

Holds the page at the next statement it runs. While paused the page runs nothing, so waits block: resume first. Browser only, as set-breakpoint.

Usage: `wf browser debug-pause [--device N] [--timeout ms]`

Examples:

```sh
wf browser debug-pause --timeout 2000
```

<details><summary>Full help</summary>

```text
Usage: wf browser debug-pause [--device N] [--timeout ms]

Holds the page at the next statement it runs. While paused the page runs nothing, so waits block: resume first. Browser
only, as set-breakpoint.

Result: {paused: true, reason, function, file, line, column, frames: [...], locals: {...}, action}, or, when no
JavaScript ran within the timeout (default 5000): {paused: false, pending: true, waitedMs, hint}
```

</details>

### `wf browser debug-step`

One step from the pause (default over). Browser only, as set-breakpoint.

Usage: `wf browser debug-step [over|into|out] [--device N] [--timeout ms]`

Examples:

```sh
wf browser debug-step over
```

<details><summary>Full help</summary>

```text
Usage: wf browser debug-step [over|into|out] [--device N] [--timeout ms]

One step from the pause (default over). Browser only, as set-breakpoint.

Result: the new location, as debug-pause answers it
```

</details>

### `wf browser debug-eval`

Evaluates an expression in the paused top frame; a throw fails (exit 1). Browser only, as set-breakpoint.

Usage: `wf browser debug-eval <expr> [--device N]`

Examples:

```sh
wf browser debug-eval ticks
```

<details><summary>Full help</summary>

```text
Usage: wf browser debug-eval <expr> [--device N]

Evaluates an expression in the paused top frame; a throw fails (exit 1). Browser only, as set-breakpoint.

Result: {expression, value, type, function, file, line}
```

</details>

### `wf browser debug-resume`

Releases the paused page, or cancels a pending pause. Browser only, as set-breakpoint.

Usage: `wf browser debug-resume [--device N]`

Examples:

```sh
wf browser debug-resume
```

<details><summary>Full help</summary>

```text
Usage: wf browser debug-resume [--device N]

Releases the paused page, or cancels a pending pause. Browser only, as set-breakpoint.

Result: {resumed, paused: false}
```

</details>

### `wf browser tailnet-forward`

Forwards one tailnet host:port to a local port through the daemon's tailnet access, so a page in the device frame can reach it; a second forward to the same target reuses the first. The daemon starts when none runs.

Usage: `wf browser tailnet-forward --host H --port P [--local-port L]`

Examples:

```sh
wf browser tailnet-forward --host leetcode-backend-prod --port 8789 --local-port 8789
```

<details><summary>Full help</summary>

```text
Usage: wf browser tailnet-forward --host H --port P [--local-port L]

Forwards one tailnet host:port to a local port through the daemon's tailnet access, so a page in the device frame can
reach it; a second forward to the same target reuses the first. The daemon starts when none runs.

Result: {url, localPort, reused}
```

</details>

### `wf browser tunnel`

Publishes the deployed VM services (vm-ops-api, remote-chat-agent, leetcode-backend, leetcode-capture; all by default) on localhost ports (remote port + 10000) through tailscaled's userspace SOCKS5 listener, then prints the env for a deployed-service suite and stays in the foreground until Ctrl-C (a backgrounded tunnel serves a stale mapping after the VM is recreated). A port already in use is reported as already forwarding.

Usage: `wf browser tunnel [services...]`

Examples:

```sh
wf browser tunnel vm-ops-api
wf browser tunnel remote-chat-agent leetcode-backend
```

<details><summary>Full help</summary>

```text
Usage: wf browser tunnel [services...]

Publishes the deployed VM services (vm-ops-api, remote-chat-agent, leetcode-backend, leetcode-capture; all by default)
on localhost ports (remote port + 10000) through tailscaled's userspace SOCKS5 listener, then prints the env for a
deployed-service suite and stays in the foreground until Ctrl-C (a backgrounded tunnel serves a stale mapping after the
VM is recreated). A port already in use is reported as already forwarding.
```

</details>

### `wf browser start`

Starts the browser daemon for this checkout, or reuses the running one. Frozen for @lib/browser-client and the cookie-capture service: it prints the URL as text, or with --json one pino line {level, time, pid, hostname, result: {url, mode}, msg}.

Usage: `wf browser start [--headless] [--screenshot-dir D]`

Examples:

```sh
wf browser start --headless
```

<details><summary>Full help</summary>

```text
Usage: wf browser start [--headless] [--screenshot-dir D]

Starts the browser daemon for this checkout, or reuses the running one. Frozen for @lib/browser-client and the
cookie-capture service: it prints the URL as text, or with --json one pino line {level, time, pid, hostname,
result: {url, mode}, msg}.
```

</details>

### `wf browser status`

The running daemon's URL and sessions, or exit 1 when none runs. Frozen as start: with --json one pino line carrying result: {url, activeBrowserName, browserCount, browsers}.

Usage: `wf browser status`

Examples:

```sh
wf browser --json status
```

<details><summary>Full help</summary>

```text
Usage: wf browser status

The running daemon's URL and sessions, or exit 1 when none runs. Frozen as start: with --json one pino line carrying
result: {url, activeBrowserName, browserCount, browsers}.
```

</details>

### `wf browser stop`

Stops the daemon and every session in it, or exits 1 when none runs. Frozen as start.

Usage: `wf browser stop`

Examples:

```sh
wf browser stop || echo "no browser daemon was running"
```

<details><summary>Full help</summary>

```text
Usage: wf browser stop

Stops the daemon and every session in it, or exits 1 when none runs. Frozen as start.
```

</details>

## cache

### `wf cache`

The build cache: one OCI Object Storage bucket holds every cache layer, and the sandbox and CI share it

Usage: `wf cache [options] [command]`

<details><summary>Full help</summary>

```text
Usage: wf cache [options] [command]

The build cache: one OCI Object Storage bucket holds every cache layer, and the
sandbox and CI share it. Nothing is cached on the VM, so a recreated VM finds it
all. A store that is unreachable, over budget or refused makes a run cold, never
red.

Layers (one prefix, one expiry and one verb each; `docs/solutions/ci/cache-layers.md`):
  nx        Nx task results and declared outputs   nx/        14 days   wf cache nx
  sccache   Rust compile output                    sccache/   14 days   wf cache sccache
  pnpm      the downloaded npm packages            pnpm/      60 days   wf cache restore|save pnpm
  metro     Metro's transform cache and tsc's      metro/     14 days   wf cache restore|save metro
            build info, one restore-latest object

`wf test run --stage` and `wf check --staged` start a loopback shim that gives
Nx the bucket (NX_SELF_HOSTED_REMOTE_CACHE_SERVER, a token per run, and
127.0.0.1,localhost added to NO_PROXY) and stop it when Nx exits. It needs
BUILD_CACHE_READ_PAR_URL and BUILD_CACHE_WRITE_PAR_URL (the environment, then
the vault); when either does not resolve nothing starts and Nx uses its local
cache alone, as it does under `wf test run --fresh`. The same shim also serves Rust
compiles when `sccache` is on PATH: it adds RUSTC_WRAPPER=sccache, CARGO_INCREMENTAL=0
(sccache refuses incremental compiles), a WebDAV endpoint on the loopback and its
own server port to the Nx child, and prints `wf: compile cache {...}` and
`wf: sccache {hits, misses}` when it stops. A compile's key includes the target
directory path, so keep CARGO_TARGET_DIR the same between runs. WF_BLOB_CACHE=off, or
"enabled": false in clis/blob-cache/data/budget.json, turns the cache off.

A run writes only when the tree is clean (untracked files count; under
`wf check --staged` what is staged does not), HEAD is an ancestor of origin/dev,
and node and pnpm are the versions mise.toml pins. Otherwise one stderr line
gives the reason (`dirty`, `not-on-dev`, `tools-unpinned`) and the run reads
only. The caps are in clis/blob-cache/data/budget.json.

Every run ends with one stderr line, `wf: build cache <json>`:
  {"hits": n, "misses": n, "puts": n, "skipped": n, "quotaExceeded": n, "droppedPuts": n,
   "lastFailure": null | {"class": "unreachable" | "credential" | "rejected" | "quota-exceeded", "status": n | null},
   "requests": n, "bytes": n,
   "degraded": null | "unreachable" | "credential" | "budget",
   "latencyMs": {"get": T, "put": T, "exists": T}}   T = {"n", "sumMs", "maxMs"}
hits and misses count reads, puts the entries written, skipped the calls that
sent nothing (cold, read-only, too large, or an entry already there), quotaExceeded
the writes the tenancy's full Object Storage refused (HTTP 400 StorageQuotaExceeded;
one extra stderr line `wf: build cache storage quota exceeded` flags it; `wf cache
guard` makes room), requests
and bytes what the run spent against its cap, degraded why the run went cold.
latencyMs is how long the store took to answer the calls that went out: n of
them, their total and the slowest, in milliseconds (a put is the whole call,
with its existence check). droppedPuts counts the writes that were wanted and did
not land (a read-only run is not one: its refusal has its own event), lastFailure the
class and HTTP status of the last store call that failed, never the store's words.

Each operation also leaves one structured event on stderr (logfmt, source `ci-cache`):
`cache.restore` / `cache.save` (pnpm, metro, with `error_class` and `error_status` when
the store failed), `cache.run` (one per shim layer, with seeded_checked and seeded_present: how many of its writes the store holds when asked again), `cache.write.refused` (warn: why the
run reads only, and for a dirty tree the paths), `cache.put.dropped` (warn),
`cache.shim.disabled` (warn: why the shared cache is off, also printed as a line),
`gate.seeding`, `nx.stage.summary`, `drift.report`. The same steps send gauges to the
Better Stack source named by BETTERSTACK_METRICS_HOST and BETTERSTACK_METRICS_TOKEN
(labels as scope, name): ci.cache.requests{kind,result}, ci.cache.step_seconds{kind,verb},
ci.cache.degraded{kind,reason}, ci.cache.seeded{kind,checked|present}, ci.nx.tasks{stage,outcome}, ci.nx.replay_ratio{stage},
ci.gate.seeded{stage}, ci.drift.tasks_drifted. Best effort: at most 2 seconds, never
a failed run; WF_CI_METRICS=off turns them off.

The verbs print one JSON object on stdout, documented under each verb; none
prints a PAR link. `status`, `gc`, `guard` and `purge` are the operator's (`guard`
also runs in CI). `restore` and `save` keep the pnpm store in the bucket: the CI jobs run `restore` before and
`save` after `pnpm install --frozen-lockfile`, and neither fails a job.

Options:
  -h, --help                  display help for command

Commands:
  nx <command...>             Run a command with Nx served from the bucket
  sccache <command...>        Run a command with Rust compiles served from the
                              bucket (needs sccache on PATH)
  status [options]            Bytes stored and requests this month against the
                              caps; exits 1 above either
  gc [options]                Delete cache objects: --evict-to <bytes>,
                              --prefix <p>, or --status
  guard [options]             Keep the tenancy's Object Storage under its cap:
                              evict above 85 percent down to 70, never fails
  purge <kind>                Start a new empty generation (every kind starts
                              over; the old entries expire by lifecycle)
  restore <kind>              Fill an empty local pnpm store (pnpm) or app build
                              directory (metro) from the bucket
  save <kind>                 Upload the pnpm store after a green install, or
                              the app build directory after a build
  help [command]              display help for command
```

</details>

#### `wf cache nx`

Run a command with Nx served from the bucket

Usage: `wf cache nx [options] <command...>`

Examples:

```sh
wf cache nx -- pnpm exec nx run mla-tamagui:typecheck
```

<details><summary>Full help</summary>

```text
Usage: wf cache nx [options] <command...>

Run a command with the Nx task cache served from the bucket: a loopback server
starts, NX_SELF_HOSTED_REMOTE_CACHE_SERVER and a per-run token go into the
command's environment (and 127.0.0.1,localhost into NO_PROXY), and the server
stops when the command exits. The command's exit code is this verb's. Needs
BUILD_CACHE_READ_PAR_URL and BUILD_CACHE_WRITE_PAR_URL (the environment, then the
vault); without them the command runs on Nx's local cache alone. A run writes only
from a clean tree on origin/dev with pinned tools (see `wf cache --help`).

With --why (first, before the command), every task Nx ran in the command is
explained against the bucket's history of that task (verification/nx-inputs/):
the nearest earlier run and the inputs that differ (a file, an env var by name and
never by value, a runtime tool), `expired` (an earlier run stored this very hash),
or `never-stored` (no earlier run, or the run that computed the hash failed and
stored nothing); this run's own inputs are recorded for the next miss. A task that
failed says it stored nothing. Writing needs VERIFICATION_ARTIFACTS_PAR_URL.

Output: the command's own, then one stderr line `wf: build cache {...}` and, with
--why, one `wf: nx why {schema, ok, run, source, misses: [{task, hash, verdict,
because, against: {run, sha, ts, source, hash, outcome, passed}, changed: [{input,
kind, change, name|files|was|now}], changedCount, stored, thisRun?}], recorded,
problems}`.

Arguments:
  command     the command to run, after an optional --

Options:
  --why       explain each task the command ran against its earlier runs
  -h, --help  display help for command
```

</details>

#### `wf cache sccache`

Run a command with Rust compiles served from the bucket

Usage: `wf cache sccache [options] <command...>`

Examples:

```sh
wf cache sccache -- cargo test -p wf-kit
```

<details><summary>Full help</summary>

```text
Usage: wf cache sccache [options] <command...>

Run a command with Rust compiles served from the bucket. A loopback WebDAV server
starts (sccache's backend uses one endpoint, the bucket has a read link and a
write link, so the server routes between them), and RUSTC_WRAPPER=sccache,
CARGO_INCREMENTAL=0, SCCACHE_WEBDAV_ENDPOINT, SCCACHE_WEBDAV_TOKEN and a free
SCCACHE_SERVER_PORT go into the command's environment. When the command exits the
run's own sccache server is stopped and its hit and miss counts printed. The
command's exit code is this verb's. Needs sccache on PATH and the two links; without
either the command runs with uncompiled-cache semantics and one line says why.

A hit costs one request, a miss one read and one write; the run is capped by
`sccache.requestsPerRun` and `bytesPerRun` in clis/blob-cache/data/budget.json.
Keep CARGO_TARGET_DIR the same between runs: a compile's key includes it.

Output: the command's own, then stderr lines `wf: compile cache {gets, puts, store}`
and `wf: sccache {hits, misses, writeErrors}`.

Arguments:
  command     the command to run, after an optional --

Options:
  -h, --help  display help for command
```

</details>

#### `wf cache status`

Bytes stored and requests this month against the caps; exits 1 above either

Usage: `wf cache status [options]`

Examples:

```sh
wf cache status --json
```

<details><summary>Full help</summary>

```text
Usage: wf cache status [options]

What the build cache holds and has used against its caps: the bytes stored in the
bucket and the requests this month, against clis/blob-cache/data/budget.json.
Exits 1 above either cap. Needs the operator's `oci` and ~/.oci, so it runs in
the sandbox; nothing unattended depends on it.

The bytes are the bucket's approximate size (`oci os bucket get`); the requests
are OCI Monitoring's AllRequests for the dedicated bucket, summed from the first
of this month (UTC), which lags by about a day.

Output: one JSON object on stdout, with or without --json: {ok, storedBytes,
storedBytesCap, objects, requestsThisMonth (null when the store reports none),
requestsPerMonthCap, layers, over: ["storedBytes" | "requestsPerMonth"]}. ok is false
when over is not empty. layers is one row per cache layer (nx, pnpm, probe, sccache,
metro) with {bytes, objects, ttlDays} from one object listing, or null when the
listing is unavailable. A failure (no oci, no key, a call refused) prints
{ok: false, error} and exits 1.

Options:
  -h, --help  display help for command
```

</details>

#### `wf cache gc`

Delete cache objects: --evict-to <bytes>, --prefix <p>, or --status

Usage: `wf cache gc [options]`

Examples:

```sh
wf cache gc --evict-to 1500000000
```

<details><summary>Full help</summary>

```text
Usage: wf cache gc [options]

Delete cache objects. The bucket's lifecycle rules expire entries by age on
their own; this is for an overrun. Needs the operator's `oci` and ~/.oci. Give
exactly one option. The generation pointer (meta/generation) is never deleted by
--evict-to, nor by a --prefix that does not start with meta/: losing it would
send every reader back to generation 0.

Output: one JSON object on stdout {ok, deleted, freedBytes, storedBytes}, the
bytes counted from the object listing; ok is false, with exit 1, when --evict-to
could not reach its target. With --status {ok, storedBytes, objects}, counted the
same way, and nothing is deleted. A failure (no oci, no key, a call refused)
prints {ok: false, error} and exits 1; a malformed line exits 2.

Options:
  --status            report what is stored and delete nothing
  --evict-to <bytes>  delete the oldest objects, by creation time, until the
                      bucket holds at most this many bytes
  --prefix <p>        delete every object whose name starts with <p> (not empty)
  -h, --help          display help for command
```

</details>

#### `wf cache guard`

Keep the tenancy's Object Storage under its cap: evict above 85 percent down to 70, never fails

Usage: `wf cache guard [options]`

Examples:

```sh
wf cache guard
```

<details><summary>Full help</summary>

```text
Usage: wf cache guard [options]

Keep the tenancy's Object Storage under its cap. Every bucket (build-cache,
verification-artifacts, pnpm-cache, goldens, tfstate) shares one 20 GB quota
(clis/blob-cache/data/budget.json tenancyStoredBytes, held equal to
infra/oracle-cloud/always-free-quota.tf by a test), and a full tenancy rejects
every write with StorageQuotaExceeded. Above 85 percent of the cap the guard
deletes until usage is under 70 percent, cheapest first and oldest first within a
class: sccache/, metro/, probe/, nx/, the old pnpm stores (never the newest store
and its sidecar), verification test-runs bundles older than 3 days, then eas-updates
and device-lab older than 3 days. It never touches tfstate, goldens, libs, meta/,
latest-<branch> pointers, the update group a pointer names (read from the pointer;
when one cannot be read no update group goes), or any object younger than 1 day.
A delete names the version the object was listed with, so one rewritten since is
left alone. Evicted test-runs bundles stay listed in verification/test-runs/index.json:
whatever follows an index entry must tolerate a 404.

The CI runner holds no OCI credential. Without --local the guard asks agent-shell,
which does, to run `wf cache guard --local` through vm-ops-api's exec route (the
signed access `wf vm exec` and the deploy legs use) and waits at most 45 seconds.
--local uses the `oci` CLI and ~/.oci of the machine it runs on, writing
~/.oci/config from the OCI_* vault secrets when it is missing.

It never fails: one JSON line on stdout and exit 0 on every path, a store that
cannot be reached included. Every listing, read and delete stops at a 30 second
limit on the machine that runs it, so the line is printed before its shell's
timeout. The plan job of pipeline.yml (vm flavor) starts it in the background and shows the
line in the run summary. A skipped line on run after run is a broken guard (no
oci CLI, no ~/.oci, an old wf on agent-shell), not noise.

Output: {event: "storage-guard", ok, action, usedBytes, usedBytesAfter, capBytes,
highWaterBytes, lowWaterBytes, deletedObjects, freedBytes, buckets: {name: bytes},
layers: {bucket: {layer: bytes}}, evicted: {class: {objects, bytes}},
changedSinceListing, wouldDelete?: {objects, bytes, listed: [{bucket, name, bytes,
class}]}, skipCode?, skip?}.
action is none (under the high-water mark), evicted (back under the low-water
mark), short (nothing more may be deleted, a delete failed or the time ran out),
would-evict (--dry-run over the high-water mark) or skipped (the tenancy could not
be measured; ok is false). skipCode is one of unmeasurable, config-missing,
listing-failed, pointer-unreadable, delete-failed, time-limit, relay-unreachable,
relay-timeout, relay-no-result; skip is the cause with key blocks and passphrases
removed. deletedObjects and freedBytes count what the store confirmed deleted.
Usage is what the listings of the build-cache and verification-artifacts buckets
add up to (a bucket's own size figure lags a delete by hours); the other buckets
count their approximate size.

Options:
  --local     run here with this machine's oci CLI instead of on agent-shell
  --dry-run   decide as a real run would and delete nothing (action would-evict)
  -h, --help  display help for command
```

</details>

#### `wf cache purge`

Start a new, empty generation (every kind starts over; the old entries expire by lifecycle)

Usage: `wf cache purge [options] <kind>`

Examples:

```sh
wf cache purge nx
```

<details><summary>Full help</summary>

```text
Usage: wf cache purge [options] <kind>

Start a new, empty generation: every reader and writer moves to it at once, with
no list and no delete, and the old entries expire by the bucket's lifecycle. Use
it for a cached wrong result. The generation is one number in meta/generation, so
a purge starts every kind over; <kind> (nx, sccache, pnpm or metro) names what it is for and is
checked. Needs both links, BUILD_CACHE_READ_PAR_URL to read the number and
BUILD_CACHE_WRITE_PAR_URL to write the next one (the environment, then the
vault). Unlike a run it does not ask for a clean tree on origin/dev: it writes no
task result, and a purge is how a wrong one is discarded.

Output: one JSON object on stdout {ok, kind, generation}. A failure prints
{ok: false, error} and exits 1; an unknown kind exits 2. No verb prints a link.

Arguments:
  kind        nx | sccache | pnpm | metro

Options:
  -h, --help  display help for command
```

</details>

#### `wf cache restore`

Fill an empty local pnpm store (pnpm) or app build directory (metro) from the bucket

Usage: `wf cache restore [options] <kind>`

Examples:

```sh
wf cache restore pnpm
```

<details><summary>Full help</summary>

```text
Usage: wf cache restore [options] <kind>

Fill an empty local pnpm store from the bucket before `pnpm install`, or (kind metro) the app's build directory
(frontend/mla-tamagui/node_modules/.cache/app-build: Metro's transform cache and tsc's build info) before an export or a typecheck, so a
recreated VM or a fresh checkout fetches nothing it already has. It does nothing
when the local store (metro: the app-build directory) is not empty, as it is on a runner whose checkout persists, and then asks the bucket nothing. Needs only
the read link (BUILD_CACHE_READ_PAR_URL: the environment, then the vault). The
store is the path `pnpm store path` prints, so npm_config_store_dir applies. The
object is unpacked beside the store and moved into place whole, so a download or
archive that breaks leaves the store as it was.

Any problem with the bucket (no link, unreachable, no object, over budget) is a
cold run: exit 0. Only a step on this machine that failed (pnpm missing, tar
failed, no room) exits 1.

Output: one JSON object on stdout {ok, kind, restored, bytes, reason, ms} where
reason says why nothing was restored (local store not empty, no object, store
not used: no link, unreachable, credential, budget or disabled, or the failed
step) and null when the store was filled.

Arguments:
  kind        pnpm | metro

Options:
  -h, --help  display help for command
```

</details>

#### `wf cache save`

Upload the pnpm store after a green install, or the app build directory after a build

Usage: `wf cache save [options] <kind>`

Examples:

```sh
wf cache save pnpm
```

<details><summary>Full help</summary>

```text
Usage: wf cache save [options] <kind>

Upload the pnpm store as its one object (store-<layout>.tar.gz, overwritten in
place, at most 1 GiB) and then the sidecar beside it that records the lockfile.
Run it right after `pnpm install --frozen-lockfile`. It saves only when all of
these hold, and asks the bucket nothing until the first three do:
  the write guard allows it (clean tree, HEAD on origin/dev, pinned node and
    pnpm; see `wf cache --help`)
  node_modules/.pnpm/lock.yaml, which pnpm writes when an install completes, is
    the same file as pnpm-lock.yaml (so the install was a green one of this
    lockfile)
  the local store has content
  the lockfile's sha256 differs from the one in the sidecar, or the object is
    missing (it expired by lifecycle)
Needs both links (BUILD_CACHE_READ_PAR_URL and BUILD_CACHE_WRITE_PAR_URL).

Any problem with the bucket is a cold run: exit 0. Only a step on this machine
that failed (pnpm missing, tar failed) exits 1.

Output: one JSON object on stdout {ok, kind, saved, bytes, lockfileSha256,
reason, ms} where bytes is the archive's size and reason says why nothing was
saved (guard refused, no green install of this lockfile, local store empty,
lockfile unchanged since the saved object, over the size cap, store not used: no
link, unreachable, credential, budget or disabled, or the failed step) and null
when the store was saved.

Kind metro: uploads frontend/mla-tamagui/node_modules/.cache/app-build as app-build.tar.gz (at most 256 MiB,
overwritten in place) when the write guard allows it, the directory has content and its fingerprint (file names
and sizes) differs from the saved one.

Arguments:
  kind        pnpm | metro

Options:
  -h, --help  display help for command
```

</details>

## check

### `wf check`

Verify a change or the repository. One mode per run; the rest of the line goes to that mode.

Usage: `wf check [options] [sha]`

| Option | Meaning |
|---|---|
| `--staged` | the lefthook pre-commit gate: lint, simplicity budget, wf bundle freshness, affected typecheck and tests, cassette sets, secrets |
| `--budget` | the simplicity budget only |
| `--cargo-inputs` | JSON list of the packages whose test:flow or test:integration only runs cargo; fails on one whose Nx inputs lack `{ "externalDependencies": [] }` (the npm layout differs by CPU, so the gate's hash never matches CI's) |
| `--deployed <sha>` | tier 3: wait for /health to report <sha>, then probe (--base, --publish, --no-publish-eas, --dry-run, ...) |
| `--health` | repo-wide snapshot: container status, BetterStack liveness and error counts; --run-tests runs every tier, --run-expensive-e2e adds the ones that spend real resources, --list-unit-targets lists them |
| `--visual` | visual gate over changed routes: screenshots vs baselines, console errors, vision judges |
| `--drift` | fail when the generated agent config or the editor bundle is stale |

Examples:

```sh
wf check --budget
wf check --staged
wf check --health --list-unit-targets
```

<details><summary>Full help</summary>

```text
Usage: wf check [options] [sha]

Verify a change or the repository. One mode per run; the rest of the line goes to
that mode.

Arguments:
  sha                dev SHA to run tier 3 against (same as --deployed <sha>)

Options:
  --staged           the lefthook pre-commit gate: lint, simplicity budget, dead
                     references, wf bundle freshness, affected typecheck and
                     tests, cassette sets, secrets. A staged file of a cassette
                     set, of `clis/mimic`, `tooling/mimic-drivers` or the Orca
                     pin runs `wf mimic verify --all` (`--live` too when the
                     reference's code or pin changed and its toolchain is
                     installed), and a staged file under a fixture tree (a
                     directory with a `fixture-provenance.json`) runs `wf
                     mimic verify --fixtures <dir>`. Lint, typecheck, unit and flow tests run
                     as `nx run-many`, so a task whose inputs hash like an
                     earlier pass (this checkout's, or one the shared build
                     cache holds: `wf cache --help`) replays, and the line
                     `verify: nx <targets> over <n> projects: <h> of <t> tasks
                     replayed from the cache` says how many. The run writes to
                     the shared cache only when the tools match mise.toml, HEAD
                     is on origin/dev and nothing unstaged or untracked is in
                     the tree; otherwise it says why on stderr and reads only,
                     so a stale sandbox never seeds it
  --pushed           the lefthook pre-push gate: what CI would run for the
                     commits being pushed, in the stages the sandbox can run
                     (typecheck-lint-unit-tests, flow-tests, integration-tests,
                     browser-e2e-tests), through the same Nx cache, so CI
                     replays their passes. It also runs the crates' live-model
                     e2e (cli-e2e-tests) so CI replays it; a vault without the
                     model key fails the push before any stage, and only
                     WF_PUSH_CLI_E2E=0 leaves it out. A browser test that would run needs
                     the export CI builds, so the gate joins the tailnet
                     (`wf vm net up`), waits for the services the tests call,
                     bakes their addresses and the vault's gateway token into
                     `wf test prepare --web`, and stops the push naming what is
                     missing; an all-replay browser stage needs none of it. Plans
                     `--changed --base origin/dev` (`--base <ref>` changes it),
                     runs each stage with `wf test run --stage`, and exits 1
                     when one fails, which stops the push. A push to dev (read
                     from the ref lines git gives the hook) writes its passes
                     to the shared cache from a clean tree; any other push
                     only reads. The deploys, the services' e2e and the browser
                     and Android tests still run in CI
  --budget           the simplicity budget only
  --cargo-inputs     JSON list of the packages whose test:flow or test:integration
                     only runs cargo; fails on one whose Nx inputs lack
                     { "externalDependencies": [] } (the npm layout differs by
                     CPU, so the gate's hash would never match CI's)
  --deployed <sha>   tier 3: wait for /health to report <sha>, then probe
                     (--base, --publish, --no-publish-eas, --dry-run, ...)
  --health           repo-wide snapshot: container status, BetterStack
                     liveness and error counts; --run-tests runs every tier,
                     --run-expensive-e2e adds the ones that spend real
                     resources, --list-unit-targets lists them
  --visual           visual gate over changed routes: screenshots vs baselines,
                     console errors, vision judges
  --drift            fail when the generated agent config or the editor bundle
                     is stale
  --dead-refs        fail when a doc, instruction, script or manifest names a
                     path, link, package, wf command, `wf browser|android` verb,
                     Lua device:<method> or pnpm script that no longer exists
                     (goals: their verbs and methods; with --staged: as the
                     index holds it, what a commit would record)
  -h, --help         display help for command
```

</details>

## ci

### `wf ci`

What a workflow tells people about its own run: `wf ci comment` posts what failed in a red CI run on the pushed commit, `wf ci live` keeps the run's one pull request comment up to date, `wf ci tidy` frees the runner's disk, `wf ci preflight` checks first that everything the run needs is live, `wf ci audit` is the nightly audit of CI on dev (never a gate) and `wf ci cache-health` fails a run whose cache layers broke.

Usage: `wf ci [options] [command]`

Examples:

```sh
wf ci
```

<details><summary>Full help</summary>

```text
Usage: wf ci [options] [command]

What a workflow tells people about its own run: `wf ci comment` posts what
failed in a red CI run on the pushed commit, `wf ci live` keeps the run's one pull
request comment up to date, `wf ci tidy` frees the runner's disk, `wf ci preflight` checks first that
everything the run needs is live, `wf ci audit` is the nightly audit of CI on dev
(never a gate) and `wf ci cache-health` fails a run whose cache layers broke.

Options:
  -h, --help           display help for command

Commands:
  live [start]         Update the run's one comment on the pull request.
  tidy [options]       Free the runner's disk when it is filling.
  preflight [options]  Check at the start that the cache, secrets, services and
                       runner are live and valid.
  audit [options]      Nightly audit of CI on dev: one JSON verdict with the
                       findings and their next steps; never fails a push.
  cache-health [options]
                       Fail the run when a cache layer degraded, lost or
                       refused a write.
  comment [options]    Comment on the pushed commit what failed in this CI run,
                       with the cause from each failed job's log.
  help [command]       display help for command
```

</details>

### `wf ci tidy`

Free the runner's disk when it is filling.

Usage: `wf ci tidy [options]`

Examples:

```sh
wf ci tidy
wf ci tidy --dry-run
```

<details><summary>Full help</summary>

```text
Usage: wf ci tidy [options]

Frees the CI runner's disk before a run needs it. When the volume is over the
high-water mark it removes what can be built again, the cheapest to rebuild
first, until the volume is under the low-water mark:

  1. the build artifacts in a live target's `deps` directories that nothing has
     read or written for a week (the test binaries of every earlier version of
     the code; cargo never collects them)
  2. old `wf` installs under /opt/clis (the published one, `current`, and the
     newest three stay)
  3. whole cargo target directories nothing has built in for a week
  4. Rust incremental build state (the `incremental` directory of every cargo
     profile), last: the `wf` build's own keeps it at 30 seconds instead of 3
     minutes

Under the high mark it removes nothing. It never removes outside the two
directories it works in (the runner's work directory, three levels above the
checkout, and /opt/clis). Over the mark it prints what the volume holds, the biggest
directories under both, in the JSON and in the job summary, so a full disk can be
explained; under it nothing is measured (walking the runner's work volume cost
800 MB of reads and 20 s of I/O wait on every run), and the JSON carries a reason
instead of the biggest directories.

Output: one JSON line {ok, dryRun, usedPercent, highWater, lowWater,
freedBytes, afterPercent, removed: [{path, bytes, why, removed}], biggest:
[{path, bytes}]}.

Options:
  --root <dir>       the work directory to search for cargo output (default: the
                     runner's work directory)
  --installs <dir>   the directory of wf installs (default /opt/clis)
  --high <percent>   remove only at or above this use (default 70)
  --low <percent>    stop once use would be at or under this (default 60)
  --dry-run          say what would go and remove nothing
  -h, --help         display help for command
```

</details>

### `wf ci live`

Update the run's one comment on the pull request.

Usage: `wf ci live [start] [options]`

Examples:

```sh
wf ci live start
wf ci live
```

<details><summary>Full help</summary>

```text
Usage: wf ci live [start] [options]

Brings up to date the one comment a CI run keeps on its pull request: the run's
report as it stands, the same text `wf ci comment` leaves when the run ends, with
the stats of every stage done (jobs with their state, time and the step a running
job is in, the minutes per job as a vertical bar chart, what the cache hit, the
slowest tests and steps, the VM's numbers). The logs of finished jobs are kept on
the runner for the run, so an update downloads each only once.

The comment is the last on the page: the CI comments of other runs are deleted,
this run's is edited in place and made when the run has none. Every job of the CI
calls it when it starts, the preflight job first (`start` is the same, spelled for
that job), and the report job ends the run with `wf ci comment`.

Nothing is posted when the branch has no open pull request. A failure to comment
is reported on stderr and never fails the step: exit 0 either way.

Environment: GITHUB_TOKEN (actions: read, pull-requests: write),
GITHUB_REPOSITORY, GITHUB_RUN_ID and GITHUB_SHA; optional GITHUB_API_URL,
GITHUB_SERVER_URL, GITHUB_REF_NAME (the branch whose pull request it is),
WF_VM_STATS_URL and WF_CI_LOG_CACHE (the directory finished jobs' logs are kept
in, default ~/.cache/wf/ci-logs).

Output: one JSON line {ok, posted, action?, commentId?, reason?}; action is
created or updated.

Options:
  -h, --help  display help for command
```

</details>

### `wf ci preflight`

Check at the start that the cache, secrets, services and runner are live and valid.

Usage: `wf ci preflight [options]`

Examples:

```sh
wf ci preflight
wf ci preflight --skip runner
```

<details><summary>Full help</summary>

```text
Usage: wf ci preflight [options]

The first job of a CI run: stops the run in seconds, with one verdict naming
what is wrong and what to do about it, when anything the run needs from start
to finish is not live and valid. Checks run in parallel:

  workspace       no file differs from the commit (the build cache refuses
                  writes from a dirty tree)
  toolchain       the node, pnpm and Rust versions mise.toml pins are the ones
                  installed (Nx hashes them)
  env             GITHUB_TOKEN, GITHUB_REPOSITORY, GITHUB_RUN_ID, GITHUB_SHA
  secrets         every secret of the `ci` group (groups.json) is present and
                  usable (`wf secret verify --for ci`)
  build-cache     both links resolve, and a probe object is written through the
                  write link and read back through the read link
  ledger          the database answers and the ledger's tables exist (a
                  warning only: the audit skips its task checks without it)
  vm-ops-api      a signed /health answers over the tailnet (every deploy leg
                  goes through it)
  reporting-service, remote-chat-agent
                  /health answers (remote-chat-agent is a warning only)
  runner-disk     10 GB free on the checkout's volume
  runner-memory   1 GB available
  github          the token is valid and 200 API calls are left
  runner-version  the gh-runner image compose pins is not a minor behind GitHub's latest
                  runner release (a warning only: a runner left behind restarts in a
                  loop trying to update and its jobs store no log)

Prints one JSON line {ok, failed: [names], checks: [{check, ok, required,
detail, next}]} and a table in the job summary; exit 1 when a required check
failed. A check that is not required is a warning.

Environment: GITHUB_TOKEN, GITHUB_REPOSITORY, GITHUB_RUN_ID and GITHUB_SHA;
optional GITHUB_API_URL, GITHUB_STEP_SUMMARY, WF_VM_STATS_URL (reporting-service),
WF_PREFLIGHT_HEALTH (`name=url,name=url`, replaces the service checks) and
WF_BLOB_CACHE=off.

Options:
  --skip <names>  comma separated checks to leave out (workspace, toolchain,
                  env, secrets, build-cache, ledger, services, runner, github)
  -h, --help      display help for command
```

</details>

### `wf ci cache-health`

Fail the run when a cache layer degraded, lost or refused a write.

Usage: `wf ci cache-health [options]`

Examples:

```sh
wf ci cache-health
```

<details><summary>Full help</summary>

```text
Usage: wf ci cache-health [options]

Judge what the cache layers said about themselves in this CI run's job logs (the
`ci-cache` events and the `wf: build cache {...}` lines) and exit 1 when one of
these happened, so the report job that runs it goes red:

  degraded          a layer went cold: the store was unreachable, its link
                    expired, or the run's request or byte budget ran out.
  rejected-write    a write the run needed was refused or dropped: the tenancy's
                    Object Storage is full (StorageQuotaExceeded) or the store
                    said no. An entry over its per-kind size cap is a deliberate
                    skip, not a refusal.
  seeded-lost       the run wrote N entries and the store holds fewer when asked
                    again ("seeded 7 of 10").
  replayed-nothing  on a push to dev, a stage the pre-push gate runs ran tasks and
                    replayed none: the gate's entries are not the ones CI looks for.
                    The workspace root's own tasks (the typecheck a docs-only push plans)
                    are not counted: they hash every installed npm package, so a gate on
                    another install never seeds CI's hash.
  drift             a drift report in the logs found tasks hashed differently
                    (judged only when a job log carries a `drift.report` event:
                    no CI job runs `wf test drift` yet).
  pnpm-save-slow    the pnpm save took longer than 60 seconds.

A disabled shared cache is a note, not a problem. The same verdict is the "Cache
health" section of `wf ci comment` and the live pull request comment. Reads the
run from the GitHub API (GITHUB_TOKEN, GITHUB_REPOSITORY, GITHUB_RUN_ID,
GITHUB_SHA; GITHUB_EVENT_NAME and GITHUB_REF_NAME say whether it is a push to
dev).

Prints one JSON object: {"ok", "events", "unreadable", "findings": [{"kind", "job", "detail"}],
"notes", "stages": [{"job", "stage", "replayed", "tasks"}]}. Exit 0 healthy, 1
a problem. When the run cannot be read, or a job log cannot be downloaded, the
verdict says so ("ok": null with "reason", or "unreadable" and a note) and the
exit stays 0: a failure to report never fails the run.

Options:
  -h, --help  display help for command
```

</details>

### `wf ci audit`

Nightly audit of CI on dev: one JSON verdict with the findings and their next steps; never fails a push.

Usage: `wf ci audit [options]`

Examples:

```sh
wf ci audit
wf ci audit --since 1793000000 --dry-run
wf ci audit --run 77 --dry-run
```

<details><summary>Full help</summary>

```text
Usage: wf ci audit [options]

The nightly audit of CI on dev (04:00 Berlin time, .github/workflows/ci-audit.yml).
It never gates or fails a push and always exits 0: it reads the completed runs of
the CI workflow on dev since --since (default the last 24 hours), judges them and
prints one JSON verdict with the findings and, for each finding, the next step.
A run's wall time says little (a cold run and a replayed one differ by an hour),
so each unit of work is judged against itself in the ledger (HEALTH_RPT.CI_TASKS
and CI_JOBS in the autonomous database; WF_CI_LEDGER_FILE names a JSON lines
file instead). Each judged run is recorded in the ledger for the next one.

Findings (kind: what it means):

  should-have-hit   a task ran although an earlier run already produced its
                    result under the same input hash (and the task has been
                    replayed from a cache before, so it can be cached, and the
                    entry is under 13 days old): the cache did not serve it.
                    A correctness fault (hard: true).
  cache-read-only   a job whose build cache refused to write ("reads only for
                    this run"), so nothing it built is reused. Hard.
  degraded, rejected-write, seeded-lost
                    the cache layers' own health, as `wf ci cache-health` judges
                    it (a degraded store, a refused write, an entry the store
                    acknowledged but does not hold). Hard.
  pnpm-save-slow, drift, replayed-nothing
                    the other `wf ci cache-health` findings (a slow pnpm save,
                    tasks hashed differently, a seeded stage that replayed none)
  pnpm-packed-outside-flow-tests
                    a job other than WF_PNPM_SAVE_JOB (flow-tests) packed and
                    uploaded the pnpm store
  hit-ratio         a stage with at least 10 tasks over the window replayed less
                    than hitRatioFloorPercent of them
  key-drift         a task's input hash moved since the previous run although
                    `nx show projects --affected` says nothing it depends on
                    changed: something non-deterministic is in its key
  slower-same-work  the same task and hash took slowerPercent and slowerSeconds
                    more than its median in earlier runs: the machine was slow
  runner-wait       a job waited more than waitSeconds for a runner that no
                    other run was holding
  idle              idlePercent of the wall time, and idleSeconds, passed
                    between jobs with nothing running
  io-wait           the VM's I/O wait peak over the run above ioWaitPeak percent
  cpu-seconds       the VM's CPU-seconds over the run (average busy share of its
                    2 cores times the wall time) above cpuSecondsThreshold plus
                    cpuSecondsPerExtraDeploy for each deploy leg beyond the
                    first; 0 turns it off
  cpu-iowait        the CPU's own I/O wait peak over the run above cpuIowaitPeak
                    percent (from reporting-service; skipped when unreachable)
  cold-slower       on a forced cold run, a job over the median of the last four
                    cold runs plus slackPercent and slackSeconds

The thresholds are in .github/ci-audit.json (WF_CI_AUDIT_FILE names another
file; a missing key keeps its default: slowerPercent 50, slowerSeconds 30,
slackPercent 25, slackSeconds 60, floorSeconds 120, waitSeconds 600,
idlePercent 5, idleSeconds 120, ioWaitPeak 60, cpuIowaitPeak 15,
cpuSecondsThreshold 0, cpuSecondsPerExtraDeploy 0, hitRatioFloorPercent 0). They
are thresholds, not gates: the nightly agent moves them with evidence
(docs/runbooks/nightly-audit.md). A run with a failed or cancelled job is not
judged for time and tasks (its timings say nothing) but its cache health still
is, and a ledger that cannot be reached is said in the notes and skips the task
checks.

Environment: GITHUB_TOKEN (actions: read), GITHUB_REPOSITORY, GITHUB_RUN_ID and
GITHUB_SHA (the audit's own run; the judged runs come from the listing);
optional GITHUB_STEP_SUMMARY, WF_VM_STATS_URL, GITHUB_API_URL, WF_CI_LEDGER_FILE,
HEALTH_REPORTS_ORDS_BASE_URL and HEALTH_REPORTS_ORDS_PASSWORD (the environment,
then the vault), WF_PNPM_SAVE_JOB and WF_NX_BIN.

Output, one JSON line (schema 1):
  {schema, ok, since, runs: [{run, sha, cold, judged, reason, tasks, cpuSeconds,
   findings}], findings: [{kind, run, subject, hard, wasteSeconds, message,
   next}], stages: [{stage, replayed, tasks, hitRatioPercent}], threshold:
   {cpuSecondsThreshold, cpuSecondsPerExtraDeploy, hitRatioFloorPercent, file},
   notes, report}
`ok` is true when nothing was found and null (with `reason`) when the runs could
not be listed; `next` is what to do about that finding; `report` is the markdown
that goes in the job summary.

Options:
  --since <seconds>  audit the runs that started at or after this epoch time
                     (default: 24 hours ago)
  --run <id>         audit this one run instead of listing (its sha is
                     GITHUB_SHA)
  --cold             with --run: the run was a forced cold run
  --dry-run          judge and print, write no job summary and no ledger rows
  -h, --help         display help for command
```

</details>

### `wf ci comment`

Comment on the pushed commit what failed in this CI run, so whoever watches the commit reads the cause without opening the run. It lists the run's jobs from the GitHub API and reports on every run, green or red: a header with links to the commit and the run and the runner, a Jobs table (result, time, runner, cache hits, misses and puts with mean and slowest get and put latency, Nx replays, docker layers cached, EAS export, upload and served seconds, the slowest step), the flow tests' crates (replayed or ran) and the e2e units of the commit (from the file WF_CI_RESULTS names). The report goes in the job summary and as a comment on the open pull request; a red run also gets a commit comment with a row per failed job and the cause from its log. A re-run of the same run on the same runner updates its comment, another run or runner gets its own and none is overwritten; a failure to comment never fails the CI: one stderr line gives the reason and the exit code stays 0. Output: one JSON line {ok, posted, commentId?, action?, prComment?, summary?, failedJobs, reason?}.

Usage: `wf ci comment [options]`

| Option | Meaning |
|---|---|
| `--dry-run` | print the comment as the body field of the JSON line (read it with `jq -r .body`) and post nothing |

Examples:

```sh
wf ci comment
wf ci comment --dry-run | jq -r .body
```

<details><summary>Full help</summary>

```text
Usage: wf ci comment [options]

Report on this CI run, most important first, so whoever watches the commit reads
the verdict and the cause without opening the run. It lists the run's jobs from
the GitHub API and reports on every run, green or red: a title (verdict, short
sha, branch, wall time) with links to the commit and the run; when a job failed,
a block per failed job (the step that failed, how long it took, a link, and the
log lines that say why) and one line naming the jobs skipped because of it; the
counts (passed, failed, skipped, build cache hit rate, docker layers cached);
"Where the time went" (wall time split into jobs running and waiting between them,
the longest jobs, runner waits from created to started, and the slowest steps,
tests, test suites and Nx tasks, read from the job logs' timestamps); a
Jobs table (result, job, time with a bar against the longest job, notes: cache
hit rate, Nx replays, docker layers cached, EAS export, upload and served
seconds, the slowest step); the VM during the run (CPU, memory, I/O wait,
memory pressure, load, swap and the docker call: average, peak and when, and
whether the VM stopped sampling), read from reporting-service over the tailnet
(WF_VM_STATS_URL, default http://reporting-service:8791; a line says why when it
cannot be read), then per job CPU, I/O wait and memory, and the containers that
used the VM, biggest disk writers first; a Cache section (build cache, Nx replays,
sccache and docker layers per job and for the run, and the tasks that ran for lack
of a hit, slowest first); the e2e units of the commit (from the file WF_CI_RESULTS
names, written by `wf test results --json`); and collapsed blocks with the slowest
steps per job, slowest tests, test suites, Nx runs (critical path, recoverable
time), deploy phases, the cache hits, misses, puts and latencies per job and the
flow tests' crates (replayed or ran).

The report goes in the job summary ($GITHUB_STEP_SUMMARY) and as THE CI comment
of the pull request of the branch: the only one, last on the page, which is this
run's own (`wf ci live` keeps it up to date while the run executes, with the
stats of every stage so far; this is the final report); the CI comments of
earlier runs are deleted. When a job failed it also goes as a commit comment,
with a collapsed block per failed job holding up to 40 more log lines. The
charts are vertical bar charts, which a phone in portrait draws whole.

"What went wrong" is read from the job's log: the ##[error] lines, cargo test
lines ending FAILED and panicked at, the tasks Nx lists as failed, a flow or e2e
unit with a fail verdict and its bundle link, docker build lines starting with
#N ERROR, JSON {ok: false} errors and HTTP errors (`wf ship`'s returned HTTP 502
with its exitCode and stderr), and `wf: ...` errors; each line loses its
timestamp and colour and is cut at 300 characters.
Anything shaped like a token (ghp_, github_pat_), a bearer value or a
pre-authenticated URL (/p/<id>/n/ paths, signed query values) is redacted. The
comment stays under 60000 characters (GitHub caps one at 65536): the details are
cut first.

A re-run of the same run updates its comment in place, found by the marker line
<!-- ci-live run:<id> -->; the CI comments of other runs on the pull request are
deleted, and no comment of anyone else is touched. A failure
to comment never fails or masks the CI result: one stderr line gives the
reason and the exit code stays 0.

Environment: GITHUB_TOKEN (contents: write to comment, actions: read for the
jobs and their logs, pull-requests: write and issues: write for the pull
request comment), GITHUB_REPOSITORY, GITHUB_RUN_ID and GITHUB_SHA; optional
RUNNER_NAME (the marker and header), GITHUB_STEP_SUMMARY, WF_CI_RESULTS and
WF_VM_STATS_URL; optional
GITHUB_API_URL (default https://api.github.com), GITHUB_SERVER_URL (default
https://github.com) and GITHUB_REF_NAME (the branch in the header). The CI
workflow's report job sets them.

Output: one JSON line on stdout {ok, posted, commentId?, action?, prComment?,
summary?, failedJobs, reason?}: posted, action and commentId describe the commit
comment (only a red run makes one; action is created or updated), prComment is
{number, action, commentId} or {error} or null without an open pull request,
summary says whether the job summary was written, failedJobs the names of the
failed jobs (empty when the run is green); ok is false, with posted false
and the reason, when an environment variable is missing or the jobs, the comment
list or the post could not be reached (a log that cannot be read only says so in
that job's row). With --dry-run, {ok, posted: false, dryRun: true, failedJobs,
body}.

Options:
  --dry-run   print the comment as the body field of the JSON line (read it
              with `jq -r .body`) and post nothing
  -h, --help  display help for command
```

</details>

## device commands

The device verbs of `wf browser` and `wf android`, Maestro's commands in kebab-case, generated from the command table (`clis/harness-core/src/device/api.rs`): `wf <noun> <verb> --help` prints one in full, `wf <noun> test --api` the Lua side (`--json` the table itself). Each prints one JSON line: `{ok, command, platform, device, ms, result}` and exit 0; a failure `{ok:false, error, evidence, verdict, met_after_ms, next}` and exit 1; a usage error exit 2; an Android lease conflict exit 3. The browser's own verbs (devices, hierarchy, check-taps, the debugger, the tailnet and the daemon) are under [browser](#browser), Android's (devices, hierarchy, lease, adb) under [android](#android).

| Verb | Maestro | Platforms | Result |
|---|---|---|---|
| `open-link` | openLink | both | {link} |
| `tap-on` | tapOn | both | {x, y, bounds} |
| `swipe` | swipe | both | {start, end, duration} |
| `scroll-until-visible` | scrollUntilVisible | both | {bounds, text, swipes} |
| `input-text` | inputText | both | {typed, ms} |
| `press-key` | pressKey | both | {key} |
| `back` | back | android only | {} |
| `extended-wait-until` | extendedWaitUntil | both | visible: the element {bounds, text}; not_visible: true; condition: its first truthy value; an optional miss: null (CLI: {met, value, ms, checks}) |
| `assert-visible` | assertVisible | both | the element {bounds, text}; an optional miss: null (CLI: {met, value, ms, checks}) |
| `assert-not-visible` | assertNotVisible | both | true; an optional miss: null (CLI: {met, value, ms, checks}) |
| `take-screenshot` | takeScreenshot | both | {path, width, height} |
| `assert-screenshot` | assertScreenshot | both | {path, verdict, match_percentage} |
| `start-recording` | startRecording | both | {path} |
| `stop-recording` | stopRecording | both | {files} |
| `get-element` | (ours) | both | {bounds: {left, top, right, bottom, cx, cy}, text} or null |
| `get-device-state` | (ours) | both | {screen: {width, height, dpr}, keyboard: {visible, bounds}, microphone: {input, consumed}}, or the one part asked for |
| `get-pixel` | (ours) | both | {r, g, b} |
| `get-logs` | (ours) | both | ["<level>: <message> (at <frames>)", ...] |
| `clear-logs` | (ours) | both | {cleared} |
| `set-microphone-input` | (ours) | both | {path, bytes} |
| `draw-strokes-on` | (ours) | both | {strokes, points} |
| `run-shell` | (ours) | android only | {stdout, exit_code} (Lua: stdout, exit_code) |

Lua only: `device:add_network_stub`, `device:get_network_stub_hits`, `device:get_debug_hits`.


## exec

### `wf exec`

The executor of an rca workspace (docs/plans/rca-client/architecture.md). It runs

Usage: `wf exec [options] [command]`

<details><summary>Full help</summary>

```text
Usage: wf exec [options] [command]

The executor of an rca workspace (docs/plans/rca-client/architecture.md). It runs
at the workspace, connects out to the rca service and performs every action a
model takes there: run a command, read, write, edit, list, search and cancel,
each confined to --root (docs/plans/rca-client/protocol.md). The agent CLIs run
in the rca service; this is the only part that touches the workspace.

This is the separate binary rca-exec: `wf exec ...` runs `rca-exec exec ...` in
its place, found beside wf, else on PATH; `rca-exec serve ...` works too.

Every line on stdout is one JSON object and no verb prompts. A failure prints
{ok: false, error, next} and exits 1; a malformed line exits 2.

Options:
  -h, --help                  display help for command

Commands:
  serve [options]             Attach to rca and serve a workspace until the
                              channel closes
  help [command]              display help for command
```

</details>

### `wf exec serve`

Attach this directory to the rca service as an executor: open

Usage: `wf exec serve --rca-url <url> --workspace <id> --root <dir> [--executor <id>] [--worktree-base <dir>] [--browse-allow <dirs>] [--reconnect] [--git-hooks] [--secret-env NAME]`

<details><summary>Full help</summary>

```text
Usage: wf exec serve --rca-url <url> --workspace <id> --root <dir> [--executor <id>] [--worktree-base <dir>] [--browse-allow <dirs>] [--reconnect] [--git-hooks] [--secret-env NAME]

Attach this directory to the rca service as an executor: open
GET /exec/v2/executors?workspace=<id> (a WebSocket, Authorization: Bearer) and
run every request frame {id, tool, args} against --root, answering
{id, ok, result} or {id, ok: false, error: {code, message}}; run_command also
streams {id, stream, data} frames first, and worktree_setup streams its progress
as {id, seq, event} frames. A path outside --root and --worktree-base (a `..`,
or a symlink that leaves the directory it started in) is PATH_OUTSIDE_ROOT; only
browse reads elsewhere, inside --browse-allow. A second executor attached with
the same id replaces this one: rca closes this channel with code 4001. When the
channel ends, whatever still runs for it is killed.

Options:
  --rca-url <url>             The rca service, http://host:port or ws://host:port
  --workspace <id>            The executor's root workspace; a turn names it
  --root <dir>                The repository root every action is confined to
  --executor <id>             The executor's id (default: the workspace id); rca
                              keeps the workspaces made later (workspace.create,
                              worktree.create) with the executor that owns them
  --worktree-base <dir>       Where git_worktree_add makes worktrees, a directory
                              outside --root; confinement widens to it
  --browse-allow <dirs>       Comma-separated directories browse may list besides
                              --root and --worktree-base
  --reconnect                 When the channel drops (not a close with 4001), open
                              it again for the same executor: its ptys go on and
                              rca re-attaches to them by byte offset
  --git-hooks                 Let the repository's own hooks run in the git actions
                              (commit, merge, rebase, checkout, ...); off by default
  --secret-env <NAME>         The environment variable holding the bearer (default
                              CHAT_AGENT_API_SECRET); the value is never printed
  -h, --help                  display help for command

Output, one JSON object per line: {ok: true, event: "attached", workspace,
executor, root, worktreeBase, rcaUrl} once the channel is open, then on a clean
close {ok: true, event: "closed", closeCode} and exit 0. A refused connection, an
unset bearer, a bad --root, --worktree-base or --browse-allow, a dropped socket
or a close with 4001 prints {ok: false, error, next} (with closeCode for 4001)
and exits 1. A missing option exits 2.
```

</details>

## flow

### `wf flow`

The flow harness: vitest over the app's *.flow.test.tsx (harness doubles, jsdom), the third runner beside `wf browser test` and `wf android test`.

Usage: `wf flow <run|goldens> [options]`

Examples:

```sh
wf flow run
wf flow goldens --snapshots .flow-goldens/light/snapshots --goldens tests/goldens --color-scheme light
```

<details><summary>Full help</summary>

```text
Usage: wf flow <run|goldens> [options]

The flow harness: vitest over the app's *.flow.test.tsx (harness doubles, jsdom), the third runner beside
`wf browser test` and `wf android test`.

  run [<test file>]   run the flow tests (all, or one file) and write .flow-report/{report.json,index.html,debug.jsonl};
                      --publish uploads the run as a bundle; --json prints {verdict, exitCode, tests, failed, files,
                      durationMs, report, debug}
  goldens             paint the frozen pages a flow run wrote and compare them with the goldens (`wf flow goldens --help`)
```

</details>

### `wf flow run`

Runs the flow tests of frontend/<app> (vitest, vitest.flow.config.ts, --configLoader runner), all of them or one file, with FLOW_DEBUG_OUT pointed at .flow-report/debug.jsonl so the tests' logAt/breakAt stops are recorded. Flow tests are TypeScript: there is no --script here; a logpoint or breakpoint is a line in the test (`logAt(file, line, [exprs])`, `breakAt(...)` from @lib/flow-test-harness). Ends with "report: <.flow-report/index.html>"; --publish uploads the report dir as a run bundle (`wf test publish`) and posts its test_runs row. Exit code is vitest's.

Usage: `wf flow run [<test file>] [--file <test file>] [--app mla-tamagui] [--goldens | --update-goldens]`

Examples:

```sh
wf flow run --update-goldens
wf flow run --goldens
```

<details><summary>Full help</summary>

```text
Usage: wf flow run [<test file>] [--file <test file>] [--app mla-tamagui] [--goldens | --update-goldens]
                   [--theme light|dark|both] [--publish] [--json]

Runs the flow tests of frontend/<app> (vitest, vitest.flow.config.ts, --configLoader runner), all of them or one file,
with FLOW_DEBUG_OUT pointed at .flow-report/debug.jsonl so the tests' logAt/breakAt stops are recorded. Flow tests are
TypeScript: there is no --script here; a logpoint or breakpoint is a line in the test (`logAt(file, line, [exprs])`,
`breakAt(...)` from @lib/flow-test-harness). Ends with "report: <.flow-report/index.html>"; --publish uploads the report
dir as a run bundle (`wf test publish`) and posts its test_runs row. Exit code is vitest's.

Screenshots and goldens, as in Flutter's golden tests: every test's last screen, and every screenshot(name) /
session.screenshot(name) it takes, is frozen as HTML and painted by Chromium after vitest (`wf flow goldens`, phone
size), so the report shows each one. Without a flag nothing is compared and only the light theme is painted. --goldens
compares every screenshot with tests/goldens/<suite>/<name>.<theme>.png and exits 1 on a difference or a missing golden;
--update-goldens writes them. --theme light|dark|both (default: light, or both with a goldens flag). With one test file
only that file's goldens are touched; a full --update-goldens run also removes goldens no test produces any more. Each
theme ends with "goldens (<theme>): <.flow-goldens/<theme>/report.html>".
```

</details>

### `wf flow goldens`

The flow harness's golden pass, with no device (`wf flow run` calls it after vitest): renders each <snapshots>/<suite>/<name>.html (the frozen DOM a flow test wrote) in headless Chromium, offline, keeps the top --max-height px of the page, downscales it to --max-width and compares it with <goldens>/<suite>/<name>.<scheme>.png (pixelmatch at --pixel-threshold, default 0.1; a match differs in at most --threshold of the pixels, default 0.002). A mismatch writes actual, expected and diff PNGs under <report dir>/out/; every golden gets a sidecar .json. --update rewrites the goldens and removes those no snapshot produced (not with --keep-unused); --save only paints each screenshot to <report dir>/out/<suite>/<name>.<scheme>.actual.png and compares nothing. The report is a gallery page.

Usage: `wf flow goldens --snapshots <dir> --goldens <dir> [--color-scheme light|dark] [--report <file>] [--threshold <ratio>]`

Examples:

```sh
wf flow goldens --snapshots .flow-goldens/light/snapshots --goldens tests/goldens --color-scheme light --threshold 0.0001 --report .flow-goldens/light/report.html
```

<details><summary>Full help</summary>

```text
Usage: wf flow goldens --snapshots <dir> --goldens <dir> [--color-scheme light|dark] [--report <file>] [--threshold <ratio>]
                       [--pixel-threshold <0..1>] [--width 393] [--height 852] [--max-width 360] [--max-height 2*height]
                       [--update | --save] [--keep-unused]

The flow harness's golden pass, with no device (`wf flow run` calls it after vitest): renders each
<snapshots>/<suite>/<name>.html (the frozen DOM a flow test wrote) in headless Chromium, offline, keeps the top
--max-height px of the page, downscales it to --max-width and compares it with <goldens>/<suite>/<name>.<scheme>.png
(pixelmatch at --pixel-threshold, default 0.1; a match differs in at most --threshold of the pixels, default 0.002).
A mismatch writes actual, expected and diff PNGs under <report dir>/out/; every golden gets a sidecar .json.
--update rewrites the goldens and removes those no snapshot produced (not with --keep-unused); --save only paints
each screenshot to <report dir>/out/<suite>/<name>.<scheme>.actual.png and compares nothing. The report is a gallery page.

Output: {"ok","scheme","report","counts":{match,updated,mismatch,missing,errors},"unused":[..],"errors":[..],
"goldens":[{"name","verdict","ok","message","golden","actual","expected","diff","differing_pixels","ratio"}],"next"}
```

</details>

## log

### `wf lib`

Versioned shared libraries: a service adopts a new version of a library when it is

Usage: `wf lib [options] [command]`

<details><summary>Full help</summary>

```text
Usage: wf lib [options] [command]

Versioned shared libraries: a service adopts a new version of a library when it is
ready, so changing a library no longer redeploys every service. libraries.json
(repo root) names the libraries and the registries they publish to and resolve
from; a registry is data there, never a code or manifest rewrite:
  {"registries": [{"alias", "kind", "url", "tokenEnv", "publish", "indexUrl"}],
   "libraries":  [{"name", "kind", "path", "registry"}]}
kind is cargo-sparse, crates-io, npm or npm-tarball. url may be `env:NAME`, read
from that variable (the environment, then the vault), because a pre-authenticated
link is a credential and is never printed or committed. tokenEnv names the
variable holding the publish credential. indexUrl (bucket registries) is the literal
base of the bucket's public sparse index (`https://objectstorage.<region>.oraclecloud.com/n/<ns>/b/<bucket>/o/index/`),
not a secret: Cargo records it in Cargo.lock, and the index is read from it with a plain GET.
tokenEnv is then only the bucket's write link. publish names the adapter that releases
there: bucket-sparse (our bucket: a static Cargo sparse registry),
cargo-publish (`cargo publish --registry <alias>`: crates.io and any registry with
Cargo's publish API), or null for a read-only registry. npm-publish and
npm-tarball are named but not built yet.

The bucket registry (alias mla) is a public bucket (read without listing) that holds only the
index at index/ (config.json and one file per crate) and the .crate files at
crates/<name>/<version>.crate. The libraries are public source, so a release publishes only
files git tracks, committed, from a HEAD origin/dev already holds, with no credential-shaped
string in them; anything else stops the release before it uploads. Readers (Cargo, `status`)
fetch it with a plain GET (indexUrl); Cargo's config.json sends downloads to
<bucket>/crates/. Writers (`release`, `prune`) write with LIBS_INDEX_WRITE_PAR_URL (a
pre-authenticated write link ending in /o/). `prune` also needs the operator's `oci` CLI and ~/.oci.

Every verb prints one JSON object on stdout and never prompts. A failure prints
{ok: false, error, next} and exits 1; a malformed line exits 2.

Options:
  -h, --help                  display help for command

Commands:
  release <library> <bump>    Bump the crate, publish it, write its index
  status                      Per library: the latest release and the version
                              each service pins
  prune [options]             Delete released versions nothing on dev pins
  retarget <library> <alias>  Point every consumer of a library at a registry
  help [command]              display help for command
```

</details>

### `wf lib release`

Release a library from libraries.json: <bump> is major, minor or patch. The

Usage: `wf lib release [options] <library> <bump>`

<details><summary>Full help</summary>

```text
Usage: wf lib release [options] <library> <bump>

Release a library from libraries.json: <bump> is major, minor or patch. The
crate's [package] version is raised, the library's registry adapter publishes it,
and when that fails the manifest is put back. A version is never replaced: a
second release of a version is refused before anything is touched, and a fix is
a new version.

bucket-sparse: `cargo package` the crate, upload crates/<name>/<version>.crate
with an if-absent write, append the version to the crate's sparse index file
(index/1/a, index/2/ab, index/3/a/abc, index/ab/cd/abcd...; the current file is read
with a plain GET) and write index/config.json, all in the one public bucket. config.json's
`dl` is <bucket base>/crates/{crate}/{version}.crate, derived from indexUrl; it holds no
bucket link and no api field. Before anything is touched the release refuses: a HEAD that
origin/dev does not hold, a file of the package that git does not track or that differs
from HEAD, and any file with a credential-shaped string (a private key, a GitHub, AWS or
Slack token, a JWT, an Oracle id): the bucket is public, so it may hold only what the
public repository holds. After a release, commit and push the bumped manifest before the next.
cargo-publish: `cargo publish --registry <alias>` with the registry and its token
passed in the environment of that one child. A library whose own dependencies on
other libraries are still paths is refused by cargo packaging: release in
dependency order.

Output: one JSON object on stdout {ok, crate, previous, version, bump, registry,
publisher, objects, checksum, bytes, warnings}; objects are the keys written
(bucket-sparse), checksum the sha256 of the .crate. cargo-semver-checks is not
run yet; the warning says so. A failure prints {ok: false, error, next}.

Arguments:
  library   a name in libraries.json (rust kind)
  bump      major, minor or patch

Options:
  -h, --help  display help for command
```

</details>

### `wf lib status`

Per library: what its registry holds and the version each service pins, read

Usage: `wf lib status [options]`

<details><summary>Full help</summary>

```text
Usage: wf lib status [options]

Per library: what its registry holds and the version each service pins, read
from every tracked Cargo.lock of the working tree (a library's own workspace
lockfile is the library, not a service). Reads only; the index is read with a plain GET
at the registry's public indexUrl, so no link and no tailnet are needed.

Output: one JSON object on stdout:
  {ok, error, libraries: [{name, kind, path, registry, latest, released, note,
    pins: [{service, lockfile, version, source, behind}]}]}
latest is the highest version not yanked, null when the registry is not read by
status or holds nothing; released lists every version in the index. source is
`path` (not migrated yet), `mla` or `crates-io`; behind counts the released
versions newer than the pin, null for a path pin. A registry URL is never
printed. ok is false, with exit 1, when the store could not be read (error says
why); the pins are still listed.

Options:
  -h, --help  display help for command
```

</details>

### `wf lib prune`

Delete released versions from the bucket registry. A version is deleted only if

Usage: `wf lib prune [options]`

<details><summary>Full help</summary>

```text
Usage: wf lib prune [options]

Delete released versions from the bucket registry. A version is deleted only if
no Cargo.lock at origin/dev names it (any lock entry of that crate and version,
path entries included), it is not the latest version of its crate, and it is
older than the grace period. Never the latest, never a pinned one. A deleted
version is first marked yanked in the index, so a new resolution never picks it,
then its .crate is removed. A registry whose adapter cannot delete (crates.io) is
skipped and listed with the reason. It runs `git fetch origin dev` first, because
a stale ref could hide a pin; --no-fetch skips that. Needs the operator's `oci` CLI
and ~/.oci, and LIBS_INDEX_WRITE_PAR_URL (the yank is written to index/ through it).

Output: one JSON object on stdout {ok, dryRun, graceDays, deleted | wouldDelete:
[{crate, version, bytes, ageDays}], freedBytes, kept: [{crate, version, reason}],
skipped: [{library, registry, why}]}. reason is latest, pinned or grace.

Options:
  --dry-run           print what would be deleted and delete nothing
  --grace-days <n>    minimum age in days (default 30)
  --no-fetch          do not fetch origin/dev first
  -h, --help          display help for command
```

</details>

### `wf lib retarget`

Point every consumer of a library at a registry named in libraries.json. In each

Usage: `wf lib retarget [options] <library> <alias>`

<details><summary>Full help</summary>

```text
Usage: wf lib retarget [options] <library> <alias>

Point every consumer of a library at a registry named in libraries.json. In each
tracked Cargo.toml the dependency line gets `registry = "<alias>"`, or, for a
crates-io registry, loses it (a line left as just a version becomes `name = "1.2.3"`).
Path and git dependencies and lines that inherit from the workspace are left and
listed. The `[registries.<alias>] index` entry of each changed workspace's
.cargo/config.toml is regenerated from libraries.json, written literally from
indexUrl (the bucket's public index, so no credential); a registry with no indexUrl whose url is
`env:NAME` (a credential) gets nothing written and the output says which
CARGO_REGISTRIES_<ALIAS>_INDEX to build with. libraries.json records the new
alias. Idempotent. npm libraries and .npmrc are not handled yet.

Output: one JSON object on stdout {ok, library, registry, previous, changed:
[{manifest, table, from, to}], skipped: [{manifest, table, why}], configs:
[{workspace, alias, written, file | why}]}.

Arguments:
  library   a name in libraries.json (rust kind)
  alias     a registry alias in libraries.json

Options:
  -h, --help  display help for command
```

</details>

### `wf log`

Emit structured log lines to BetterStack (`wf log emit`, also shipped as the static `logger` binary)

Usage: `wf log [options] [command]`

Examples:

```sh
wf log
```

<details><summary>Full help</summary>

```text
Usage: wf log [options] [command]

Emit structured log lines to BetterStack (`wf log emit`, also shipped as the
static `logger` binary)

Options:
  -h, --help      display help for command

Commands:
  emit [options]  Print one structured log line and send it to BetterStack;
                  exits 1 when it could not be sent.
  help [command]  display help for command
```

</details>

### `wf log emit`

Print one structured log line (stderr for warn and error) and send it to BetterStack; the last stdout line is {"sent":...}, and it exits 1 with the reason when the line could not be sent.

Usage: `wf log emit [options]`

| Option | Meaning |
|---|---|
| `--service <name>` | BetterStack app / OTel service.name, e.g. cloud-init |
| `--message <message>` | static event name, e.g. redeploy.started |
| `--level <level>` | log level (info&#124;debug&#124;warn&#124;error) (default: "info") |
| `--host-label <label>` | VM host label (light/heavy), attached as metadata |
| `--attr <key=value>` | repeatable structured metadata field (default: {}) |

Examples:

```sh
wf log emit 
```

<details><summary>Full help</summary>

```text
Usage: wf log emit [options]

Print one structured log line (stderr for warn and error) and send it to
BetterStack; the last stdout line is {"sent":...}, and it exits 1 with the
reason when the line could not be sent.

Options:
  --service <name>      BetterStack app / OTel service.name, e.g. cloud-init
  --message <message>   static event name, e.g. redeploy.started
  --level <level>       log level (info|debug|warn|error) (default: "info")
  --host-label <label>  VM host label (light/heavy), attached as metadata
  --attr <key=value>    repeatable structured metadata field (default: {})
  -h, --help            display help for command
```

</details>

## bench

### `wf bench agent-proof`

Agent proof: an agent gets a fixed four-step task (open <base>/leetcode/two-sum, wait for the Run Local button, tap it, wait for the outcome totals and assert they say "passed"; the autofocused editor's keyboard is lowered with Done first) plus the full `wf <harness> test --api` text, answers with one Lua script, and the script runs recorded: `wf <p> test <lua> --env base=<url>`. Pass = the script exits 0. Exit 0 when it passed, 1 otherwise.

Usage: `wf bench agent-proof --arm <name> --base <url> [options]`

| Option | Meaning |
|---|---|
| `--arm <name>` | sonnet, opencode or scripted |
| `--base <url>` | the app's web root, e.g. http://127.0.0.1:8810/mla-tamagui |
| `--out <dir>` | where <arm>-answer.txt, <arm>.lua, <arm>-run.log, <arm>-waits.jsonl and the <arm>-rec/ recording go (default: .agents/tmp/agent-proof/<ms>) |
| `--platform <name>` | browser (default) or android |
| `--script <file>` | the scripted arm's script |
| `--timeout <s>` | the agent's time limit (default: 600) |

Examples:

```sh
wf bench agent-proof
```

<details><summary>Full help</summary>

```text
Usage: wf bench agent-proof --arm <name> --base <url> [options]

Agent proof: an agent gets a fixed four-step task (open <base>/leetcode/two-sum,
wait for the Run Local button, tap it, wait for the outcome totals and assert
they say "passed"; the autofocused editor's keyboard is lowered with Done
first) plus the full `wf <harness> test --api` text, answers with one Lua
script, and the script runs recorded: `wf <p> test <lua> --env base=<url>`.
Pass = the script exits 0. Exit 0 when it passed, 1 otherwise.

Arms:
  sonnet     claude -p --model sonnet --output-format json (no tools, no MCP,
             outside the repo); cost and tokens from its JSON
  opencode   the cheap-model ladder (`wf offload ask`'s rotation, read-only, caller
             agent-proof); the first rung that answers
  scripted   no agent: runs --script (default
             e2e/agent-proof.lua), the floor

Options:
  --arm <name>       sonnet, opencode or scripted
  --base <url>       the app's web root, e.g. http://127.0.0.1:8810/mla-tamagui
  --out <dir>        where <arm>-answer.txt, <arm>.lua, <arm>-run.log,
                     <arm>-waits.jsonl and the <arm>-rec/ recording go
                     (default: .agents/tmp/agent-proof/<ms>)
  --platform <name>  browser (default) or android
  --script <file>    the scripted arm's script
  --timeout <s>      the agent's time limit (default: 600)
  -h, --help         display help for command

Prints one JSON object:
  {"arm": string, "model": string, "agentS": number, "runS": number,
   "wallS": number, "passed": boolean, "costUsd"?: number, "tokens"?: number,
   "script": string, "recording": string|null, "error"?: string}
agentS is the agent's answer time (0 for scripted), runS the harness run's time,
wallS their sum. Every budgeted device wait (assert_visible, extended_wait_until) of the run is one line
{"call","sel","ms","ok","budgetMs","platform"} in <out>/<arm>-waits.jsonl
(WF_WAIT_LOG=<file> does the same for any `wf android test` or `wf browser test`).
```

</details>

#### `wf bench cli`

Measure CLI cold start, wall time and peak RSS per command (python3 rusage, fresh process per run) plus bundle and node_modules footprint. Set WF_BIN to bench another binary.

Usage: `wf bench cli [options]`

| Option | Meaning |
|---|---|
| `--runs <n>` | runs per target (default: "5") |
| `--no-footprint` | skip the bundle and node_modules footprint (counting node_modules takes about a minute on a full install) |
| `--live` | also run commands that touch the network or the VM |
| `--out <file>` | also write the JSON report to this file |

Examples:

```sh
wf bench cli --deps
```

<details><summary>Full help</summary>

```text
Usage: wf bench cli [options]

Measure CLI cold start, wall time and peak RSS per command (python3 rusage,
fresh process per run) plus bundle and node_modules footprint. Set WF_BIN to
bench another binary.

Options:
  --runs <n>      runs per target (default: "5")
  --no-footprint  skip the bundle and node_modules footprint (counting
                  node_modules takes about a minute on a full install)
  --live          also run commands that touch the network or the VM
  --out <file>    also write the JSON report to this file
  -h, --help      display help for command
```

</details>

### `pnpm --filter mla-tamagui run build:editor-bundle`

Rebuild the code editor's CodeMirror bundle (cm6Bundle.ts) from cm6Bundle.src.ts; --check fails when the committed one is stale

Usage: `pnpm --filter mla-tamagui run build:editor-bundle [options]`

| Option | Meaning |
|---|---|
| `--check` | Only report whether the committed bundle matches a fresh build |

Examples:

```sh
pnpm --filter mla-tamagui run build:editor-bundle [--check]
```

<details><summary>Full help</summary>

```text
Usage: pnpm --filter mla-tamagui run build:editor-bundle [options]

Rebuild the code editor's CodeMirror bundle (cm6Bundle.ts) from
cm6Bundle.src.ts; --check fails when the committed one is stale

Options:
  --check     Only report whether the committed bundle matches a fresh build
  -h, --help  display help for command
```

</details>

## knowledge

### `wf knowledge`

docs/solutions knowledge index (rank, index, stale) and the Gemini Gem context bundle (index --gemini)

Usage: `wf knowledge [options] [command]`

Examples:

```sh
wf knowledge index --gemini --dry-run
```

<details><summary>Full help</summary>

```text
Usage: wf knowledge [options] [command]

docs/solutions knowledge index (rank, index, stale) and the Gemini Gem context
bundle (index --gemini)

Options:
  -h, --help                display help for command

Commands:
  rank [options] <files...>  Rank docs/solutions learnings relevant to these
                             files
  stale [options]           List learnings not touched in --days
  index [options]           Rebuild the docs/solutions index; with --gemini, build
                            one ARCHITECTURE.md + repomix bundle and upload it
                            over one Drive file for the Gemini Gem
  help [command]            display help for command
```

</details>

#### `wf knowledge index`

Rebuild the docs/solutions index; with --gemini, build one ARCHITECTURE.md + repomix bundle and upload it over one Drive file for the Gemini Gem

Usage: `wf knowledge index [options]`

| Option | Meaning |
|---|---|
| `--write` | write the docs/solutions index file instead of printing it |
| `--gemini` | build the Gemini Gem context bundle (repomix --compress skeleton, ARCHITECTURE.md drafted by `wf offload ask`, the full repomix pack, one upload) |
| `--remote <dest>` | --gemini: <drive-folder-id>/<file-name> (credential: the gcloud login with Drive access) or local:<dir>/<file-name> (default: $GEMINI_CONTEXT_REMOTE) |
| `--prompt <path>` | --gemini: the ARCHITECTURE.md prompt (default: "docs/gemini-context/architecture-prompt.md") |
| `--cache-dir <path>` | --gemini: gitignored scratch dir for the skeleton, the pack and the bundle (default: ".gemini-context-cache") |
| `--manifest <path>` | --gemini: committed record of the last upload (default: "docs/gemini-context/manifest.json") |
| `--no-summary` | --gemini: reuse the cached ARCHITECTURE.md, skip the model |
| `--dry-run` | --gemini: build the bundle, skip the upload and the manifest |

Examples:

```sh
wf knowledge index --gemini --dry-run
wf knowledge index --gemini --no-summary --remote local:/tmp/gem/repomix-context.md
```

<details><summary>Full help</summary>

```text
Usage: wf knowledge index [options]

Rebuild the docs/solutions index; with --gemini, build one ARCHITECTURE.md +
repomix bundle and upload it over one Drive file for the Gemini Gem

Options:
  --write             write the docs/solutions index file instead of printing it
  --gemini            build the Gemini Gem context bundle (repomix --compress
                      skeleton, ARCHITECTURE.md drafted by `wf offload ask`, the
                      full repomix pack, one upload)
  --remote <dest>     --gemini: <drive-folder-id>/<file-name> (credential: the
                      gcloud login with Drive access) or local:<dir>/<file-name>
                      (default: $GEMINI_CONTEXT_REMOTE)
  --prompt <path>     --gemini: the ARCHITECTURE.md prompt (default:
                      "docs/gemini-context/architecture-prompt.md")
  --cache-dir <path>  --gemini: gitignored scratch dir for the skeleton, the
                      pack and the bundle (default: ".gemini-context-cache")
  --manifest <path>   --gemini: committed record of the last upload (default:
                      "docs/gemini-context/manifest.json")
  --no-summary        --gemini: reuse the cached ARCHITECTURE.md, skip the model
  --dry-run           --gemini: build the bundle, skip the upload and the
                      manifest
  -h, --help          display help for command
```

</details>

#### `wf knowledge rank`

Rank docs/solutions learnings relevant to these files

Usage: `wf knowledge rank [options] <files...>`

| Option | Meaning |
|---|---|
| `--limit <n>` | max hits |

Examples:

```sh
wf knowledge rank
```

<details><summary>Full help</summary>

```text
Usage: wf knowledge rank [options] <files...>

Rank docs/solutions learnings relevant to these files

Options:
  --limit <n>  max hits
  -h, --help   display help for command
```

</details>

#### `wf knowledge stale`

List learnings not touched in --days

Usage: `wf knowledge stale [options]`

| Option | Meaning |
|---|---|
| `--days <n>` | age threshold in days |

Examples:

```sh
wf knowledge stale
```

<details><summary>Full help</summary>

```text
Usage: wf knowledge stale [options]

List learnings not touched in --days

Options:
  --days <n>  age threshold in days
  -h, --help  display help for command
```

</details>

## offload

### `wf offload`

Work on the cheap-model ladder, and the ladder's administration

Usage: `wf offload [options] [command]`

Examples:

```sh
wf offload
```

<details><summary>Full help</summary>

```text
Usage: wf offload [options] [command]

Work on the cheap-model ladder, and the ladder's administration

Options:
  -h, --help       display help for command

Commands:
  ask [options] [prompt]   Run one prompt on the cheap-model ladder; --batch,
                           --runbook, --stats, --savings
  stats [options]          The run-log summary of the ladder (ask --stats)
  plan [options] <slug>    Run docs/plans/<slug>/units.jsonl on git-leased
                           worktrees; --check, --status
  worker                   The git-state worker: check, status
  ladder                   The ladder's administration: clear-cooldown,
                           sync-pricing
  sessions                 The sessions the rca service keeps: rm
  bearers                  The rca service's device bearers: mint, list, revoke
```

</details>

### `wf offload ask`

Run one prompt on the cheap-model ladder (rca): an argument, --prompt, --prompt-file, or - for stdin.

Usage: `wf offload ask [options] [prompt]`

| Option | Meaning |
|---|---|
| `--stats` | ok rate, p50 and cost per shape, tier, model and pool |
| `--savings` | what delegation saved |
| `--batch` | run a prompt template over a glob |
| `--runbook` | run a runbook |

Examples:

```sh
wf offload ask --runbook persistent-session-services
wf offload ask and the Anthropic credential policy
wf offload ask --savings
```

<details><summary>Full help</summary>

```text
Usage: wf offload ask [options] [prompt]

Run one prompt on the cheap-model ladder (rca): an argument, --prompt,
--prompt-file, or - for stdin.

Options:
  --stats           ok rate, p50 and cost per shape, tier, model and pool
  --savings         what delegation saved
  --batch           run a prompt template over a glob
  --runbook         run a runbook
  -h, --help        display help for command
```

</details>

### `wf offload stats`

The run-log summary of the cheap-model ladder (the same as `wf offload ask --stats`)

Usage: `wf offload stats [options]`

Examples:

```sh
wf offload stats --json
```

<details><summary>Full help</summary>

```text
Usage: wf offload stats [options]

The run-log summary of the cheap-model ladder: okRate, p50 and cost per shape,
tier, model and pool (the same as `wf offload ask --stats`)

Options:
  --json      output machine-readable JSON lines
  -h, --help  display help for command
```

</details>

### `wf offload plan`

Run docs/plans/<slug>/units.jsonl on git-leased worktrees (the git-state worker); --check only validates the plan, --status shows each unit.

Usage: `wf offload plan [options] <slug>`

| Option | Meaning |
|---|---|
| `--check` | validate units.jsonl and print the dependency order, dispatch nothing |
| `--status` | plan status per unit (done&#124;leased&#124;failed&#124;blocked&#124;runnable) from git refs |
| `--max <n>` | max units in flight |
| `--site <site>` | sandbox&#124;vm (default: "sandbox") |
| `--land-ref <ref>` | branch units land on (default: "dev") |
| `--once` | take one unit, then exit |
| `--worktree-dir <dir>` | worktree root, outside the repo |

Examples:

```sh
wf offload plan --check device-lab
wf offload plan --site sandbox
wf offload plan vm-puppeteer
```

<details><summary>Full help</summary>

```text
Usage: wf offload plan [options] <slug>

Run docs/plans/<slug>/units.jsonl on git-leased worktrees (the git-state
worker); --check only validates the plan, --status shows each unit.

Options:
  --check               validate units.jsonl and print the dependency order,
                        dispatch nothing
  --status              plan status per unit
                        (done|leased|failed|blocked|runnable) from git refs
  --max <n>             max units in flight
  --site <site>         sandbox|vm (default: "sandbox")
  --land-ref <ref>      branch units land on (default: "dev")
  --once                take one unit, then exit
  --worktree-dir <dir>  worktree root, outside the repo
  -h, --help            display help for command
```

</details>

### `wf offload sessions`

The sessions the rca service keeps for `wf offload ask`

Usage: `wf offload sessions [options] [command]`

Examples:

```sh
wf offload sessions
```

<details><summary>Full help</summary>

```text
Usage: wf offload sessions [options] [command]

The sessions the rca service keeps for `wf offload ask`

Options:
  -h, --help       display help for command

Commands:
  rm [options]     Drop every session a caller made
  help [command]   display help for command
```

</details>

### `wf offload sessions rm`

Drop every session a caller made

Usage: `wf offload sessions rm [options]`

Examples:

```sh
wf offload sessions rm --caller e2e-run-1
```

<details><summary>Full help</summary>

```text
Usage: wf offload sessions rm --caller <name>

Drops every session the caller `<name>` made on the rca service (the `--caller`
of `wf offload ask`): each is deleted from opencode, which gives its memory
back, and its kept directory is removed. A test that is done with its sessions
says so instead of leaving them to the service's expiry (three hours idle).

Prints one JSON line {dropped: n}; exit 0, 1 when the service refuses or cannot
be reached, 2 without --caller.

Options:
  --caller <name>  the caller whose sessions to drop (required)
  -h, --help       display help for command
```

</details>

### `wf offload bearers`

The device bearers of the rca service's agent API socket

Usage: `wf offload bearers [options] [command]`

Examples:

```sh
wf offload bearers
```

<details><summary>Full help</summary>

```text
Usage: wf offload bearers [options] [command]

The device bearers of the rca service's agent API socket

Options:
  -h, --help        display help for command

Commands:
  mint [options]    Mint a device bearer and print its token, once
  list              List the device bearers that were not revoked
  revoke <id>       Revoke a device bearer
  help [command]    display help for command
```

</details>

### `wf offload bearers mint`

Mint a device bearer and print its token, once

Usage: `wf offload bearers mint --label <name>`

Examples:

```sh
wf offload bearers mint --label t3
```

<details><summary>Full help</summary>

```text
Usage: wf offload bearers mint --label <name>

Mints a device bearer on the rca service and prints its token: the one credential
a mapper (wf t3 serve) or a phone opens the agent API socket (/agent/v1/socket)
with. The service secret opens that socket no longer; it only mints, lists and
revokes, and is read from CHAT_AGENT_API_SECRET in the environment. The token is
printed here and nowhere else: the service keeps a digest of it. Store it in the
vault under the key of the app that holds it (T3_RCA_BEARER for wf t3 serve).

Prints one JSON line {ok, id, label, createdAt, token, next}; exit 0, 1 when the
service refuses or cannot be reached ({ok: false, error, next}), 2 without --label.

Options:
  --label <name>  what the bearer is for, shown in the list (required)
  -h, --help      display help for command
```

</details>

### `wf offload bearers list`

List the device bearers that were not revoked

Usage: `wf offload bearers list`

Examples:

```sh
wf offload bearers list
```

<details><summary>Full help</summary>

```text
Usage: wf offload bearers list

Lists the device bearers of the rca service that were not revoked, oldest first:
their id, label and creation time, never a token.

Prints one JSON line {ok, bearers: [{id, label, createdAt}]}; exit 0, 1 when the
service refuses or cannot be reached ({ok: false, error, next}).

Options:
  -h, --help  display help for command
```

</details>

### `wf offload bearers revoke`

Revoke a device bearer

Usage: `wf offload bearers revoke <id>`

Examples:

```sh
wf offload bearers revoke bearer-1a2b3c4d5e6f
```

<details><summary>Full help</summary>

```text
Usage: wf offload bearers revoke <id>

Revokes the device bearer with that id (from `wf offload bearers list`): its token
opens the agent API socket no longer, from the next connection on. A socket that
is open stays open until it closes.

Prints one JSON line {ok, revoked: <id>}; exit 0, 1 when the service refuses, has
no such bearer or cannot be reached ({ok: false, error, status, next}).

Options:
  -h, --help  display help for command
```

</details>

### `wf offload ladder`

The cheap-model ladder's administration

Usage: `wf offload ladder [options] [command]`

Examples:

```sh
wf offload ladder sync-pricing
wf offload ladder clear-cooldown minimax
```

<details><summary>Full help</summary>

```text
Usage: wf offload ladder [options] [command]

The cheap-model ladder's administration

Options:
  -h, --help       display help for command

Commands:
  clear-cooldown <pool>  Drop a pool from the cooldown list
  sync-pricing           Refresh the pricing table from the OpenRouter model list
  help [command]         display help for command
```

</details>

#### `wf offload ladder clear-cooldown`

Drop a pool from the cooldown list so the ladder tries it again

Usage: `wf offload ladder clear-cooldown [options] <pool>`

Examples:

```sh
wf offload ladder clear-cooldown
```

<details><summary>Full help</summary>

```text
Usage: wf offload ladder clear-cooldown [options] <pool>

Drop a pool from the cooldown list so the ladder tries it again

Options:
  -h, --help  display help for command
```

</details>

#### `wf offload ladder sync-pricing`

Refresh model-pricing.json from the OpenRouter model list

Usage: `wf offload ladder sync-pricing [options]`

Examples:

```sh
wf offload ladder sync-pricing
```

<details><summary>Full help</summary>

```text
Usage: wf offload ladder sync-pricing [options]

Refresh model-pricing.json from the OpenRouter model list

Options:
  -h, --help  display help for command
```

</details>

## mimic

### `wf mimic`

Characterization and differential testing against a pinned reference

Usage: `wf mimic [options] [command]`

<details><summary>Full help</summary>

```text
Usage: wf mimic [options] [command]

Characterization and differential testing against a pinned reference
(docs/plans/mimic/plan.md). `record` runs the reference through a driver over a
corpus and writes a cassette set: normalised interactions with machine-checked
provenance (the hash of the pin file, the rules and the corpus). `replay` feeds
the cassette's inputs to a candidate and diffs what it did; `diff` compares two
sets; `verify` checks a set is fresh and intact, offline; `crawl` derives the
reference's whole API and a request corpus from its pinned source; `stabilize` infers
the mask rules that make several recordings of one corpus equal; `remask` applies a
changed rules.json to a committed set and re-seals it; `import` and `export`
convert legacy CLI fixtures into an imported set and prove the conversion lossless.

This is the separate binary wf-mimic: `wf mimic ...` runs `wf-mimic mimic ...`
in its place, found beside wf, else on PATH, so a change here rebuilds
wf-mimic and not wf.

A cassette set is a directory: cassette.json (header), interactions.jsonl (one
interaction per line), rules.json (mimic-rules/1), corpus.json (mimic-corpus/1),
mimic.toml (targets), allowlist.json (optional), raw/ (never committed). A
target is a driver that speaks mimic-driver/1: JSON lines over the stdio of a
child process. {"hello":true} answers {"hello":{"driver":{name,version,
protocol,toolchain_lock_hash?},"target_version"?,"coverage"}}; {"id","op"?,
"input"} answers {"id","steps":[{dir,channel?,kind,rid?,len?,payload}],
"effects":[{type,...}],"outcome":{status:"ok"|"error",code?,reference_accepts?}}
or {"id","error"}.

mimic.toml (paths relative to the file):
  rules = "rules.json"             corpus = "corpus.json"  (both default to the file beside mimic.toml)
  allowlist = "allowlist.json"     (optional; entries may carry "pin_hash")
  [reference]                      the pinned program a record runs
  driver = "stdio"                 command = ["node", "driver.mjs"]
  cwd, env, timeout_ms             name, version, commit
  pin = "pin.json"                 version_pointer = "/version", commit_pointer
  [candidate.<name>]               the program a replay runs (same keys)
  [diff]                           breaking_paths = [...], advertised_prefixes
                                   ignore_dirs = ["c2s"]  (steps the driver was given are not compared)
  [diff.fold]                      a stream's chunks count as one list per request:
                                   channel, tag, chunk, values, once, squash (see wf mimic diff)
  [record]                         batch, write_chunk, stability_pct (the flags of wf mimic record)
  live_when_named = "why"          in [reference]: `verify --all --live` skips it, `verify <set> --live` runs it
  [record]                         allow_nondeterministic (default false), args: what `wf mimic job record`
                                   adds to the record call of the set
  [provision] / provision_file     how a runner installs the reference (wf mimic provision --help); the
                                   VM runs it for a [record <set>] commit marker (wf mimic trigger --help)

Every verb prints one JSON object on stdout with "schema": "mimic-<verb>/1",
"ok", "warnings" and "next" (the one command to run next), never prompts, and
exits 0 clean, 1 findings, 2 usage or environment error (then also "error" and
"evidence").

Options:
  -h, --help                  display help for command

Commands:
  record [options] <set>      Run the reference over the corpus and write the set
  replay [options] <set>      Run the cassette's inputs against a candidate and diff
  diff [options] <a> <b>      Diff two cassette sets
  verify [options] [set...]   Check sets are fresh and intact (offline)
  trigger [options]           Read the marker of a commit message into the capture jobs it asks for
  provision [options] <set>   Install a set's reference as its [provision] block says
  job [options] <record|replay> <set>  One capture job: chunked record, or replay and diff table
  publish [options] <set>...  Upload recorded sets to the bucket as one run
  fetch [options] <run-id|latest>  Download a published run into this checkout
  import [options] <format> <legacy-dir>  Convert legacy CLI fixtures into an imported set
  export [options] <format> <set>  Rebuild the legacy files of an imported set and check their hashes
  help [command]              display help for command
```

</details>

### `wf mimic record`

Run the [reference] of <set>/mimic.toml over every case of its corpus, twice, and

Usage: `wf mimic record [options] <set>`

<details><summary>Full help</summary>

```text
Usage: wf mimic record [options] <set>

Run the [reference] of <set>/mimic.toml over every case of its corpus, twice (a
big set: once, and a sample twice, --stability), and write the set: the masked first run is the cassette, kind "recorded", with the
pin hash, driver, rules hash, corpus hash and seed. If the two runs of any case
differ after masking nothing is written (exit 1): mask what varies with a rule in
rules.json, or record it flagged with --allow-nondeterministic. A reference with
no pin cannot be recorded (exit 2), unless --stand-in says the target is a double.

Options:
  --target <file>             mimic.toml to use (default <set>/mimic.toml)
  --seed <n>                  replace the corpus seed
  --recorded-on <where>       vm, sandbox or owner-paired (provenance, informational)
  --source <text>             a note for the provenance
  --stand-in <what>           the reference command runs a double (a scripted host), not the pinned
                              reference: the set is kind "authored", claims no pin hash, notes
                              <what> in its source, and so can never serve a parity row
  --keep-raw                  also write raw/capture.jsonl, the un-normalised first run
  --allow-nondeterministic    write the set with the unstable cases tagged "nondeterministic"
  --stability <pct>           run only this percent of the cases twice, the stability study (default 100,
                              or [record] stability_pct): chosen by a hash of seed and case id
  --batch <n>                 send n state-free cases at once (the driver answers them concurrently,
                              MIMIC_DRIVER_CONCURRENCY=n; default 1, or [record] batch)
  --write-chunk <n>           cases tagged "state:write" run after the others, n to a state of the
                              reference, which is reset between chunks; "state:fresh" cases get a
                              reset of their own (default: one state, or [record] write_chunk)
  --chunk <n>                 stop at the first chunk boundary after n newly recorded cases (ok, done
                              false, next); the progress is saved under <set>/raw/checkpoint (never committed)
  --max-minutes <n>           stop cleanly after n minutes (a decimal is fine) at the first batch boundary, with
                              the checkpoint written, and print complete false; a killed call loses at most its
                              current batch. Every call records at least one batch, so repeated calls finish
  --resume                    carry on from the checkpoint of an interrupted, chunked or timed recording; a
                              checkpoint of another corpus, rules, seed or batching refuses (exit 2): --fresh
  --fresh                     drop the checkpoint and record from the start
  -h, --help                  display help for command

Output (schema mimic-record/1):
  {schema, ok, done, complete, set, interactions, provenance: {kind, target: {name, version?,
  commit?, pin_file?, pin_hash}, driver, tool, seed, rules_hash, corpus_hash,
  recorded_at}, nondeterministic: [ids], wrote: [paths], evidence?: {raw},
  warnings, next}
  while a chunked or timed recording is unfinished: {schema, ok: true, done: false, complete: false, set, recorded,
  total, remaining, checkpoint, wrote: [], warnings, next}
```

</details>

### `wf mimic replay`

Feed the inputs of the cassette to a [candidate] of the set's mimic.toml, mask

Usage: `wf mimic replay [options] <set>`

<details><summary>Full help</summary>

```text
Usage: wf mimic replay [options] <set>

Feed the inputs of the cassette to a [candidate] of the set's mimic.toml, mask
what it did with the set's rules and diff it with what was recorded, under the
allowlist. A divergence the allowlist covers counts for nothing and is reported
with its reason; an entry bound to another pin (pin_hash) has expired. A driver
failure on one interaction is that interaction's breaking result.

A set whose corpus has a `property` generator stores each generated case with a ladder of smaller inputs of the same shape (ids
<case>.shrink.<n>, tagged shrink-of:<case>). When a property case fails, `shrunk` names for it the smallest stored input of the ladder
that still fails (`minimal`, its `input`), so the report carries the minimal case.

Severity: breaking (a missing key, a changed type or length, a changed status),
drift (an extra key, a changed value), none. Exit 0 only for none.

Options:
  --target <file>             mimic.toml to use (default <set>/mimic.toml)
  --candidate <name>          the [candidate.<name>] to run (default: the only one)
  --only <id,...>             replay these interactions only
  --allowlist <file>          replace the allowlist of mimic.toml
  --suite <name>              write $WF_TEST_OUTPUT_DIR/<name>/report.json and
                              trace.json (method-trace/1 with each call's row and diff, and its test's cassette kind)
  -h, --help                  display help for command

Output (schema mimic-replay/1):
  {schema, ok, set, candidate, severity: none|drift|breaking, summary: {total,
  clean, allowlisted, drift, breaking}, results: [non-clean interactions: {id,
  row?, severity, label, diffs: [{path, kind: missing|extra|changed|kind,
  severity, expected?, actual?, allowlisted?: {group, path, reason, phase}}],
  error?}], shrunk: [{case, minimal, input}], evidence_rows, unused_allowlist, evidence: {report, trace},
  warnings, next}
```

</details>

### `wf mimic diff`

Diff two cassette sets by interaction id: the reference before and after a pin

Usage: `wf mimic diff [options] <a> <b>`

<details><summary>Full help</summary>

```text
Usage: wf mimic diff [options] <a> <b>

Diff two cassette sets by interaction id: the reference before and after a pin
bump, or the reference and a candidate-recorded set. An id only <a> has is
breaking, an id only <b> has is drift. The rules, diff options and allowlist are
those of <a>/mimic.toml when it exists; a mimic.toml that exists but does not
load fails the diff (exit 2) instead of being ignored.

The diff options of mimic.toml [diff] shape what is compared (replay uses them
too): breaking_paths (a changed scalar there is breaking, not drift),
advertised_prefixes (a changed boolean under one is reported, never failing),
ignore_dirs (steps of those directions, e.g. "c2s", are what the driver was
given and are not compared) and [diff.fold] for a stream whose reference splits
values into chunks as it likes: the chunks of one address (channel, direction,
request id) count as one list of their `values`, a value whose `once` pointer
holds the given string counts the first time only, and each `squash` pointer's
array counts a run of equal elements once. Both sides are folded the same way.

Options:
  --allowlist <file>          replace the allowlist
  -h, --help                  display help for command

Output (schema mimic-diff/1):
  {schema, ok, a, b, severity, summary, results (as in replay), unused_allowlist,
  warnings, next}
```

</details>

### `wf mimic crawl`

Derive a reference's whole API from its pinned source, never from a hand-written

Usage: `wf mimic crawl [options] [dir...]`

<details><summary>Full help</summary>

```text
Usage: wf mimic crawl [options] [dir...]

Derive a reference's whole API from its pinned source, never from a hand-written
list. <dir>/crawl.toml names a crawl command (the driver's `crawl <app>`, which reads
the pinned tree: the contract schemas, the method registry, the call sites of the
mobile app and of the CLI, the CLI's own command specs) and writes beside it:
  inventory.json   every endpoint: id, kind, transport, who uses it (mobile, cli,
                   internal), the sources that showed it, how its input was derived,
                   the CLI commands, and the endpoint classes the crawl cannot see
                   (each with the source that would show them)
  corpus.json      mimic-corpus/1: per endpoint a valid, boundary and invalid input
                   derived from its payload schema (tag class:<valid|boundary|invalid|empty>)
  coverage.json    mimic-coverage/1: what became of each endpoint, case or skipped with why
`wf mimic record` then runs corpus.json against the real server.

The completeness guard fails (exit 1, nothing written) when the pinned source's own
listing (the committed catalog, written by `wf parity coverage`) names an endpoint the
crawl lacks ("lags-source"), an endpoint has no case and no skip reason ("uncovered"),
a schema-derived endpoint lacks a valid or an invalid case ("missing-class"), a case names
no endpoint, the inventory's class counts differ from the cases, or the crawl was made
at another commit than the pin file names ("stale-pin").

crawl.toml (paths relative to the file):
  app = "t3"                         command = ["node", "src/cli.ts", "crawl", "t3"]
  cwd = "../../../tooling/mimic-drivers"    env = { KEY = "value" }  (the child also inherits the environment:
                                     MIMIC_T3_ROOT and MIMIC_ORCA_ROOT point at the reference trees)
  catalog = "<app>-methods.json"     pin = "<pin file>"    commit_pointer = "/commit"

Options:
  --all                       every crawl directory under --root (one with a crawl.toml)
  --root <dir>                where --all looks (default the git top level)
  --check                     offline: re-run the guard over the committed files and prove
                              coverage.json and inventory.json are what inventory and corpus
                              derive; needs no reference tree
  --fresh                     with --check, also crawl again and require the committed files
                              to be byte-equal (needs the reference trees)
  -h, --help                  display help for command

Output (schema mimic-crawl/1):
  {schema, ok, sets: [{dir, app, ok, mode: crawl|check|check-fresh, summary: {app, pin,
  endpoints, by_kind, by_user, by_source, by_input, by_class, cases, skipped, streaming,
  cli_commands, sources: [{name, available, found, note?}], blind: [{class,
  missing_source, detail}]}, problems: [{code, id, detail}], warnings, changed, wrote, next,
  error?}], warnings, next}
```

</details>

### `wf mimic stabilize`

Make recordings of one corpus agree. Each <recording> is a set written by

Usage: `wf mimic stabilize [options] <recording> <recording>...`

<details><summary>Full help</summary>

```text
Usage: wf mimic stabilize [options] <recording> <recording>...

Make recordings of one corpus agree. Each <recording> is a set written by
`wf mimic record --allow-nondeterministic` against the same corpus (two or three
consecutive recordings; a copy of the set directory per run). The cases present in
all of them are compared after the first recording's rules; for each that differs:
  - a value that differs but keeps its shape (a hex or base64 id, a port, a hash, an
    ISO timestamp, a cookie name with a port and a hash, a uuid) proposes a `token`
    rule with the regex of that shape, unless a stable answer of the corpus also
    matches it: then the rule is a `pointer` rule scoped to that interaction, so a
    stable answer is never masked
  - a number under a key that differed in every recording proposes `volatile-number`
    for the key; a short number or word that differs is never masked (cause short-value)
  - the same frames of a stream in another interleaving propose `canonical-order`
    (the server-to-client steps of a channel ordered by the request they answer)
  - what no rule can honestly hide stays unstable with a cause: text-change,
    array-length, key-set, type-change, short-value, volatile
The proposals are applied and the comparison repeated until no new rule appears.

Options:
  --allowlist <file>   stability-allowlist.json (default <first recording>/stability-allowlist.json):
                       {schema: "mimic-stability-allowlist/1", entries: [{id, cause, reason}]};
                       every unstable id must be listed with the cause it still has and a reason,
                       and a listed id that is stable now is stale
  --min-pct <n>        the share of compared cases that must agree after the rules (default 99)
  --write              apply the rules to the first recording: rules.json gains the proposed
                       rules, its interactions are masked by them, the `nondeterministic` tag
                       follows what is still unstable, and <set>/stability.json is written
  --report <file>      where --write puts the report (default <first recording>/stability.json)
  -h, --help           display help for command

Output (schema mimic-stability/1):
  {schema, ok, runs: [set names], compared, unobserved: [ids missing from a run],
   stable_before, stable_after, stable_pct, min_pct, causes: {cause: count},
   proposals: [{rule: <mimic-rules/1 rule>, why, scope: "global"|<interaction id>, seen_in}],
   unstable: [{id, row, cause, reason, samples: [{pointer, cause, a, b}]}],
   accounting: {allowlisted, unlisted, stale, wrong_cause}, wrote: [paths], warnings, next}
Exit 0 when the accounting is clean and stable_pct reaches --min-pct, 1 otherwise, 2 on usage.
```

</details>

### `wf mimic remask`

Apply the current rules.json of a set to its committed interactions and re-seal its

Usage: `wf mimic remask [options] <set>`

<details><summary>Full help</summary>

```text
Usage: wf mimic remask [options] <set>

Apply the current rules.json of a set to its committed interactions and re-seal its
rules hash. A rules change makes every committed set stale (`wf mimic verify`, and the
pre-commit hook that runs it, fail on rules_hash); this is the offline way to land new
masks together with the set they were written for, until the next record replaces it.
Masking a masked value again changes nothing but what a new rule reaches, a `pointer`
rule only touches a pointer the interaction has, and the tags, the stability allowlist,
the corpus and the provenance (but the rules hash) are kept. Nothing is recorded or run.

Options:
  -h, --help           display help for command

Output (schema mimic-remask/1):
  {schema, ok, set, rules_hash: {before, after}, changed: [interaction ids whose
   masked text differs], warnings, next}
Exit 0, 2 on an unreadable set or invalid rules.
```

</details>

### `wf mimic verify`

Freshness and integrity of cassette sets. Offline by default: the cassette's

Usage: `wf mimic verify [options] [set...]`

<details><summary>Full help</summary>

```text
Usage: wf mimic verify [options] [set...]
       wf mimic verify --fixtures <dir> [--rules <file>] [--files]

Freshness and integrity of cassette sets. Offline by default: the cassette's
content hash and count, the provenance its kind must carry (a recorded set names
a pin hash, a version or commit and a driver; an authored one claims no pin), and
the pin file, rules file and expanded corpus of mimic.toml against the hashes the
cassette recorded ("ok", "stale" or "missing" each). A set whose mimic.toml is
gone is checked for integrity only, with a warning; one that exists but does not
load (bad TOML, unknown key) is a failed set with its error. Every interaction tagged
`nondeterministic` must be listed in <set>/stability-allowlist.json with its cause and a
reason, and every entry must name one (`wf mimic stabilize`): a problem otherwise.

Options:
  --all                       every set under --root (a directory with mimic.toml and cassette.json)
  --root <dir>                where --all looks (default the git top level)
  --fixtures <dir>            provenance scan of a fixture tree instead of sets (see below)
  --rules <file>              the classification rules of --fixtures (default <dir>/fixture-provenance.json)
  --files                     with --fixtures, also list every file with its kind, rule and exchange counts
  --live                      also re-record the reference into memory and diff with the
                              committed cassette (needs the reference; never cached); with
                              --all a set whose [reference] says live_when_named is skipped
                              (live.skipped names why), a named set always runs
  -h, --help                  display help for command

Output (schema mimic-verify/1):
  {schema, ok, sets: [{set, dir, ok, kind, interactions, checks: [{name: pin_hash|
  rules_hash|corpus_hash, status, recorded?, current?}], problems: [{field,
  message}], live?: {ok, severity, summary, results, nondeterministic} | {ok, skipped}, warnings,
  error?}], warnings, next}

Provenance scan (--fixtures <dir>, offline, exit 0 clean, 1 a file is unclassified, 2 usage):
every file under the rules file's "include" globs is classified by the first rule whose
"match" glob fits its path relative to <dir> (`*` and `?` stay in a segment, `**` spans
segments). kind: authored (our own contract), mirrors-reference (a claimed copy of
reference behaviour, to be re-recorded as a recorded cassette; the rule names the
parity "row") or cli (output of a real CLI, an X3 cassette). Every rule has a "reason".
A file no rule fits is a finding; a rule that fits no file is a warning.
Rules file (schema mimic-fixture-rules/1): {schema, include: [glob], rules: [{match,
kind, reason, row?}]}.
Output (schema mimic-fixtures/1): {ok, dir, rules, total: {files, authored,
mirrors_reference, cli, unclassified, with_volatile, exchanges}, groups: {<first path
segment>: <same counts>}, unclassified: [path], unused_rules: [glob], files?: [{path,
kind, rule, exchanges, volatile_pointers}], warnings, next}
```

</details>

### `wf mimic import`

Convert legacy CLI fixtures into a cassette set of kind "imported" (honest

Usage: `wf mimic import [options] <format> <legacy-dir>`

<details><summary>Full help</summary>

```text
Usage: wf mimic import [options] <format> <legacy-dir>

Convert legacy CLI fixtures into a cassette set of kind "imported" (honest
provenance: nothing here runs claude, codex, opencode, gh or glab, and a legacy
recording that claims "recorded" is refused). Formats: exec-recording
(<legacy-dir> holds recordings.json and recordings/*.jsonl), exec-hosted
(<legacy-dir> holds the hosted_*.json fixtures) and t3-golden (<legacy-dir>
holds the <scenario>.jsonl goldens of tooling/t3-conformance recorded from the
pinned T3 server; the pairing, token exchange and socket scenarios become one
"session" interaction whose input is the client's side as a driver script, the
variant ordering file a permutation of it tagged "variant:order", the timing of
each line is dropped). Each legacy file's canonical sha256 (sorted keys,
minified; the exact bytes for a .jsonl, without `t` for a t3-golden line) goes
into provenance.imported_from, so `wf mimic export` can show the conversion lost
nothing. A mimic.toml without a [reference] is written beside the set; for
t3-golden the set also gets the allowlist (copied verbatim), the T3 rules, the
differ's T3 settings and the pinned server as the [reference] of `verify --live`.

Options:
  --out <set dir>             where the set is written (required)
  --root <dir>                the paths recorded in imported_from are relative to it
                              (default the git top level)
  --allowlist <file>          t3-golden: the allowlist (default <legacy-dir>/../../allowlist.json)
  --pin <file>                t3-golden: the pin file (default <legacy-dir>/../../t3-pin.json)
  -h, --help                  display help for command

Output (schema mimic-import/1):
  {schema, ok, format, set, kind, interactions, imported_from: [{path, hash}],
  wrote, warnings, next}
```

</details>

### `wf mimic export`

Rebuild the legacy files of an imported set from its cassette and compare the

Usage: `wf mimic export [options] <format> <set>`

<details><summary>Full help</summary>

```text
Usage: wf mimic export [options] <format> <set>

Rebuild the legacy files of an imported set from its cassette and compare the
canonical sha256 of each with the one recorded in provenance.imported_from: the
proof that the import lost nothing, offline and without the legacy files. Exit 1
when a file does not rebuild to its recorded hash. With --out the rebuilt files
(canonical JSON, sorted keys) are written under that directory at their recorded
paths.

Options:
  --out <dir>                 also write the rebuilt files here
  -h, --help                  display help for command

Output (schema mimic-export/1):
  {schema, ok, format, set, files: [{path, expected, actual?, ok}], wrote,
  warnings, next}
```

</details>

### `wf mimic trigger`

Read the marker of a commit message and say which capture jobs it asks for. The

Usage: `wf mimic trigger [options]`

<details><summary>Full help</summary>

```text
Usage: wf mimic trigger [options]

Read the marker of a commit message and say which capture jobs it asks for. The
record-references workflow runs this on the head commit of every push to dev
whose message carries a marker; it is the one place the marker is parsed.

Marker grammar (one marker per message, on a line of its own that begins with `[`;
prose that mentions it mid-line, and a message without one, start nothing):
  [record <set>... [chunk=<minutes>] [fresh]]    record each set's reference on the VM runner
  [replay <set>...]                              replay each set against its candidates, publish the diff
  <set>      a cassette set: its directory name, or its path from the repository root;
             the directory holds a mimic.toml (a bare name that matches two is refused)
  chunk=<n>  minutes per `wf mimic record --max-minutes` call, 1 to 60 (default 10)
  fresh      drop the checkpoint of an earlier run: the record starts over
  At most 8 sets. A malformed marker, an unknown set, a record of a set without a
  [reference], a corpus and a [provision] block, or a replay of a set without a
  cassette and a [candidate] fails with the reason (exit 2).

Options:
  --message <text>            the commit message
  --file <path>               read it from a file, or - for stdin
  --root <dir>                the repository root the sets are searched under (default: this checkout)
  -h, --help                  display help for command

Output (schema mimic-trigger/1):
  {schema, ok, go, action: record|replay|null, chunk?, fresh?, sets: [names],
  matrix: {include: [{name, dir, provision, drivers, node, python, go, rust, checkpoint,
  candidates}]}, warnings, next}   (the workflow reads each matrix key by name)
```

</details>

### `wf mimic provision`

Install the reference of a set on this machine as the [provision] block of its

Usage: `wf mimic provision [options] <set>`

<details><summary>Full help</summary>

```text
Usage: wf mimic provision [options] <set>

Install the reference of a set on this machine as the [provision] block of its
mimic.toml says (or the file its provision_file names, which sets of one app
share: `//path` is from the repository root, any other path from the mimic.toml): the pinned source fetched at its commit, then each [[step]] in order,
each bounded by its timeout. Programs run with this environment minus every
variable that looks like a credential; their output goes to stderr. A work
directory that holds the same provisioning (same block, same commit) is reused.

Block (paths relative to the file that declares them):
  [toolchain]                     what the runner sets up first, each checked against the tool:
    node = "24"                   node, python, go (24, 22.14, 22.14.0) and rust (those or
                                  stable); a new clone of a Rust, Go or Python program pins its own
  drivers = true                  install the repository's mimic drivers (pnpm install --filter
                                  mimic-drivers...) before a record's steps; default: when node is set
  workdir = "orca"                directory under the work root (default: the set's name)
  [source]                        optional: the reference's own repository
    repo = "https://..."          https, no credentials
    commit = "<40 hex>"           or: pin = "<pin file>", commit_pointer = "/commit"
  [env]                           NAME = "value" the record sees; no credentials
  [[step]]                        name, run = ["program", "arg"...] (no shell),
    cwd, when, timeout_minutes    cwd defaults to {source} (else {work}); when is record
                                  (default), replay (builds the candidate) or always;
                                  timeout_minutes 1 to 180, default 30; its own env
  An env key that changes how programs are found or loaded (PATH, HOME, LD_*, NODE_OPTIONS, *_PROXY ...) is refused.
  Placeholders in env values, run and cwd: {repo} (the checkout), {work}, {source}, {set}.

Options:
  --when <record|replay>      which steps run (default record)
  --work-root <dir>           where work directories live (default $MIMIC_WORK_ROOT, else ~/.cache/mimic-work)
  --root <dir>                the checkout {repo} names (default: this one)
  --check                     validate the block and print what would run; install nothing
  --force                     start the work directory over
  -h, --help                  display help for command

Output (schema mimic-provision/1):
  {schema, ok, set, when, work, source?: {repo, commit, dir}, toolchain, env, key,
  steps: [names], cached?, ran?: [{name, seconds}], checked?, warnings, next}
```

</details>

### `wf mimic job`

One capture job of the record-references workflow, for a set that is already

Usage: `wf mimic job [options] <record|replay> <set>`

<details><summary>Full help</summary>

```text
Usage: wf mimic job [options] <record|replay> <set>

One capture job of the record-references workflow, for a set that is already
provisioned (wf mimic provision).

record: runs `wf mimic record <set> --resume --max-minutes <chunk>` again and again
(each call is a bounded chunk that leaves its checkpoint under <set>/raw/checkpoint)
until the record is complete or --budget is spent. A complete record has raw/
deleted, is verified offline and is scanned for token-shaped values (a Bearer
token, a JWT): any of them holds the set back (exit 1), because it is published
to a bucket anyone can read. For a set that allows nondeterminism, a verify that fails
only on stability accounting (the tags against stability-allowlist.json) does not hold
the set back: it is reported as `verify.accounting_deferred`, and `wf mimic verify` at the
commit is the check that the allowlist names the tagged ids. An incomplete one exits 1 with the checkpoint kept
and "incomplete": true; push the marker again to resume. The checkpoint is bound to
the provision block (with its source commit), the corpus and the reference pin: one
bound to anything else, or to nothing, is dropped and the record starts over (--fresh
does so on purpose). A checkpoint with a token-shaped value in it is not uploaded
("checkpoint_upload": false). A wf-mimic whose `record` lacks --resume or
--max-minutes fails at once with that message (exit 2). The [record] table of
the set's mimic.toml sets allow_nondeterministic (default false: a stability check is
kept) and extra `record` args; the [env] of the [provision] block reaches the record
(and replay) through this job alone.
replay: replays the set against each of its [candidate.<name>] (or --candidate) and
tabulates severity, counts and the first difference of every non-clean interaction;
exit 1 when any candidate differs.
Both append a markdown summary to --summary (default $GITHUB_STEP_SUMMARY).

Options:
  --chunk <minutes>           record: minutes per call, 1 to 60 (default 10)
  --budget <minutes>          record: stop starting chunks after this long (default 100)
  --fresh                     record: delete the checkpoint first
  --candidate <name>          replay: only this candidate
  --out <dir>                 replay: write <set>.replay.json here
  --work-root <dir>           where the provisioned work directories are ({work}, default $MIMIC_WORK_ROOT)
  --root <dir>                the checkout {repo} names (default: this one)
  --summary <file>            where the markdown summary is appended
  --run <id>                  the run id to name (default $GITHUB_RUN_ID, else local)
  -h, --help                  display help for command

Output (schema mimic-job/1):
  record: {schema, ok, action, set, dir, run, complete, incomplete?, checkpoint_upload?, checkpoint_leaks?, chunks: [{n, seconds, complete,
  interactions}], chunk_minutes, interactions, nondeterministic, stable_pct, wall_seconds,
  raw_removed?, verify?: {ok, exit}, leaks?: [text], error?, warnings, next}
  replay: {schema, ok, action, set, dir, run, candidates: [{candidate, severity, summary}],
  differences, wall_seconds, warnings, next}
```

</details>

### `wf mimic publish`

Upload recorded sets to the verification-artifacts bucket as one run, so another

Usage: `wf mimic publish [options] <set>...`

<details><summary>Full help</summary>

```text
Usage: wf mimic publish [options] <set>...

Upload recorded sets to the verification-artifacts bucket as one run, so another
checkout can fetch them over plain HTTPS (an agent session cannot reach the
GitHub Actions API, and an Actions artifact needs a login to download). Uploads
the json, jsonl, toml, txt and md files of each set, never raw/ or a dot file,
each under verification/record-references/<run>/<set>/, then the manifest (every
file's size and sha256), then latest/<set>.json. A set without a cassette.json is
refused. The write link is VERIFICATION_ARTIFACTS_PAR_URL (the vault, or the
environment); it is never printed. Caps: 60 MB a file, 200 MB a run.

Options:
  --run <id>                  the run id (default $GITHUB_RUN_ID, else local)
  --root <dir>                the checkout the sets' paths are recorded against (default: this one)
  --summary <file>            append the fetch and commit commands here (default $GITHUB_STEP_SUMMARY)
  -h, --help                  display help for command

Output (schema mimic-publish/1):
  {schema, ok, run, sets: [{name, dir, files: [{path, bytes, sha256}]}], files, bytes,
  fetch, warnings, next}
```

</details>

### `wf mimic fetch`

Download a published run (wf mimic publish) into this checkout: every file of

Usage: `wf mimic fetch [options] <run-id|latest>`

<details><summary>Full help</summary>

```text
Usage: wf mimic fetch [options] <run-id|latest>

Download a published run (wf mimic publish) into this checkout: every file of
every set is read from the bucket's public URL, its sha256 is checked against the
manifest, and it is written to the set's directory (paths from the checkout root,
never outside it). A file that does not match writes nothing more. `latest` takes
the newest run of --set. Then verify the set and commit it.

Options:
  --set <name>                only this set (needed for latest)
  --root <dir>                the checkout to write into (default: this one)
  -h, --help                  display help for command

Output (schema mimic-fetch/1):
  {schema, ok, run, sets: [{name, dir, files}], warnings, next}
```

</details>

## parity

### `wf parity`

How far the agent API mappers are from parity with the pinned phone apps

Usage: `wf parity [options] [command]`

<details><summary>Full help</summary>

```text
Usage: wf parity [options] [command]

How far the agent API mappers are from parity with the pinned phone apps
(docs/plans/rca-parity/plan.md section 5): every method, command type, HTTP
call and subscription the phone reaches is a row in a coverage table under
docs/plans/rca-parity/coverage/, and parity is reached when no row is pending.
It is also the standalone binary `wf-parity` (`wf-parity parity coverage ...`).

Every line on stdout is one JSON object and no verb prompts. A failed check or
a refusal prints {ok: false, ...} and exits 1; a malformed line exits 2.

Options:
  -h, --help                  display help for command

Commands:
  coverage [options]          Scan the pinned reference and write, or check,
                              the app's method list and coverage table
  track <verb> [options]      Is the pinned reference the newest release of the
                              real server: check, bump, plan
  help [command]              display help for command
```

</details>

### `wf parity coverage`

Scan the pinned phone app and build its coverage table

Usage: `wf parity coverage --app t3|orca [--check] [--traces <dir|bundle>] [--root <dir>] [--base <rev>]`

Examples:

```sh
wf parity coverage --app t3
wf parity coverage --app orca --check
```

<details><summary>Full help</summary>

```text
Usage: wf parity coverage --app t3|orca [--check] [--traces <dir|bundle>] [--root <dir>] [--base <rev>]

Scan the pinned phone app and build its coverage table. The reference is
references/t3code (pinned by tooling/t3-conformance/t3-pin.json) or
references/orca (tooling/orca-conformance/orca-pin.json). The scan follows the
phone's import graph (apps/mobile into packages/client-runtime; mobile/ into
src/shared) and counts a library method only when the phone uses its
declaration. It writes docs/plans/rca-parity/coverage/<app>-methods.json; the
table <app>.json and <app>.md are built from that list, the hand-kept
<app>-decisions.json (milestones by row id or `prefix*`, T3 command targets,
not-offered and excluded rows) and what the mapper answers today (t3 serve's
RPC, command and mutation lists and its HTTP routes). A row is served only with
evidence: a passing test of a complete integration chain run whose method-hit
trace shows the row's call answered and, for a command or mutation whose target
is agent:<method>, that method answered in the same call's window. The evidence
of record is the checked-in <app>-evidence.json (row to the test, the agent
methods and the executor tools); a row the mapper only answers stays pending,
with a note, and a row with a `reverify` note in the decisions stays pending
whatever a trace says (the mapper refuses it, or answers without doing it).
A row whose decision says `"oracle": "mimic"` is served only by a `wf mimic replay
--suite` trace: a call labelled `diff` clean or allowlisted, in a test whose
`cassette` is `recorded`; a chain call, a drift, or an authored or imported
cassette never serves it. Its evidence entry then names `oracle`, `diff` and
`cassette`, and the table's note shows them.

--traces refreshes the evidence from a chain run: the test output dir
(WF_TEST_OUTPUT_DIR, every <suite>/trace.json under it), one trace.json, or the
bundle.json `wf test publish` made of the run (a path or its URL; its `traces`
list is read). A write rewrites <app>-evidence.json and the tables from it; a
check compares the checked-in evidence with it. Without --traces the checked-in
evidence stands, so a check never needs the run. The evidence records where the
traces were read only as a URL or a path inside the repository (--traces paths
under the root are made relative); an absolute or scratch path is left out, and
a checked-in file that has one fails the check.

Without the reference checked out, the table is rebuilt from the checked-in
method list. A reference checked out at another commit than its pin is refused
for a write.

Options:
  --app <t3|orca>             The phone app
  --check                     Write nothing: fail when the checked-in files
                              (the evidence file too, with --traces) differ
                              from what is built, a row has no milestone or
                              decidedBy of the form M<n>, a T3
                              command row has no target of the form
                              agent:<method>, state:<key> or t3-local, a
                              not-offered row has no reason, a row claims
                              served without evidence, the evidence names a row
                              the table does not serve, a trace names a row the
                              phone does not reach, or pending rose
  --traces <dir|bundle>       The method-hit traces of a chain run: a test output
                              dir, a trace.json, or a bundle.json (path or URL)
  --root <dir>                The repository root (default: the git top level)
  --base <rev>                What pending is compared with (default HEAD when
                              the table is dirty, else HEAD^); a commit since it
                              whose message carries 'parity: pin-bump' may
                              raise it
  -h, --help                  display help for command

Output, one JSON object: {ok, app, mode, source: "reference-scan" or
"methods-list", reference: {dir, head, pin, atPin}, counts, summary: {rows,
served, pending, notOffered, excluded, flagged, answeredByMapper, byKind,
pendingByMilestone}, evidence: {file, rows, refreshedFrom (a URL or a repository path, else null)}, problems: [{rule,
message, rows}], wrote (write mode)}.
Rules: stale, unassigned, untargeted, incomplete-decision,
served-without-evidence, stale-evidence, unknown-trace-row, stale-decision,
pending-rose, scratch-trace-path. A write reports the
problems and exits 0; --check exits 1 on any. A refusal prints {ok: false,
app, error, next} and exits 1.
```

</details>

### `wf parity track`

Follow the real servers the phone apps are conformed to

Usage: `wf parity track check|bump --app t3|orca [--root <dir>]`

Examples:

```sh
wf parity track check --app t3
wf parity track bump --app orca
wf parity track plan --message "[track t3 orca]"
wf parity track report --plan plan.json --inputs out --out report
```

<details><summary>Full help</summary>

```text
Usage: wf parity track check|bump --app t3|orca [--root <dir>]
       wf parity track plan (--message <text> | --file <path|->) [--root <dir>]
       wf parity track report --plan <plan.json> --inputs <dir> --out <dir> [--run <id>] [--sha <commit>]

Follow the real servers the phone apps are conformed to. T3 is the npm package
`t3` (pinned in tooling/t3-conformance/t3-pin.json, its release on
github.com/pingdotgg/t3code); Orca is the newest GitHub release of
stablyai/orca (tooling/orca-conformance/orca-pin.json). No verb prompts and
every line on stdout is one JSON object (schema parity-track/1).

check   Compare the pin with the newest release. A pin that is behind is data,
        not a failure (exit 0):
        {schema, ok, app, pinned, latest, drifted, next}
bump    Write the pin at the newest release (a pin already there is left alone).
        T3: version, commit, release URL, npm integrity and the size and sha256
        of each pinned platform's tarball (from the release's SHA256SUMS).
        Orca: version and the commit of the release tag. Reports
        {schema, ok, app, pinned, latest, drifted, bumped, wrote, commit,
        rerecord: [cassette sets], stale: [what to regenerate], review: [pin
        fields only a person can set], next}.
plan    Read the [track <app>... [chunk=<minutes>] [fresh]] marker of a commit
        message (a line of its own that begins with `[track `), check each app
        it names and print {schema, ok, go, apps, chunk, fresh, drifted,
        verdicts: [the check verdicts], record_marker, next}. record_marker is
        the [record <set>...] marker of the sets of the drifted apps, for
        `wf mimic trigger --message`, or null when no pin is behind. sets lists
        the sets it names.
report  Fold a tracker run into one drift report. --inputs holds the capture
        outputs (<set>.record.json and <set>.replay.json of `wf mimic job`,
        coverage-<app>.json of `wf parity coverage`); --out gets
        drift-report.json {schema parity-track-report/1, ok, run, sha, apps:
        [{app, pinned, latest, drifted, status: current|incomplete|
        discrepancies|needs-work|clean, sets: [{set, recorded, replay}],
        coverage}], next}, summary.md, and report.json + report.html (a
        flow-report/1) that `wf test publish <out>` makes a run bundle of.
        Prints the drift report; ok is false only when a capture did not finish.

The newest release is read from the public services; WF_TRACK_NPM,
WF_TRACK_GITHUB_API and WF_TRACK_GITHUB_WEB name others, and GITHUB_TOKEN (sent
to the GitHub API only) lifts its rate limit. A service that cannot be reached
or answers differently is {ok: false, error, next} and exit 1; a line that
does not parse exits 2.

Options:
  --app <name>                t3 or orca (check, bump)
  --message <text>            the commit message (plan)
  --file <path>               read it from a file, or - for stdin (plan)
  --plan, --inputs, --out     the plan.json, the capture outputs and the report dir (report)
  --run, --sha                the workflow run and the commit the report names (report)
  --root <dir>                the repository root (default: this checkout)
  -h, --help                  display help for command
```

</details>

## agent-config

### `wf agent-config`

The .agents/ contract rendered into every tool's config

Usage: `wf agent-config [options] [command]`

Examples:

```sh
wf agent-config
```

<details><summary>Full help</summary>

```text
Usage: wf agent-config [options] [command]

The .agents/ contract rendered into every tool's config

Options:
  -h, --help       display help for command

Commands:
  sync [options]   Render the .agents/ contract into every tool config; --check
                   only reports whether the generated files are current
```

</details>

### `wf agent-config sync`

Render the .agents/ contract into every tool config

Usage: `wf agent-config sync [options]`

| Option | Meaning |
|---|---|
| `--check` | Only report whether the generated files are current |

Examples:

```sh
wf agent-config sync --check >/dev/null 2>&1 && return
wf agent-config sync >/dev/null 2>&1
```

<details><summary>Full help</summary>

```text
Usage: wf agent-config sync [options]

Render the .agents/ contract into every tool config

Options:
  --check     Only report whether the generated files are current
  -h, --help  display help for command
```

</details>

## run

### `wf run api`

`wf run` is gone (it prints the verbs to use). The script API is generated from the device command table: `wf android test --api` and `wf browser test --api` print it, `--json` the table itself.

## secret

### `wf secret`

Manage secrets (macOS Keychain locally; Bitwarden in production — pick with --backend or SECRETS_BACKEND)

Usage: `wf secret [options] [command]`

Examples:

```sh
wf secret get/env, bws secret get/list, or a variable captured from one
```

<details><summary>Full help</summary>

```text
Usage: wf secret [options] [command]

Manage secrets (macOS Keychain locally; Bitwarden in production — pick with
--backend or SECRETS_BACKEND)

Options:
  -h, --help                            display help for command

Commands:
  list [options]                        Show all registered secrets and their
                                        sources
  get [options] <KEY>                   Print raw value (exits 1 if missing)
  set <KEY> [value]                     Store secret (reads from stdin if value
                                        omitted) — macOS backend only
  delete <KEY>                          Remove secret — macOS backend only
  env [options]                         Print KEY=value lines for all available
                                        secrets (eval-safe)
  sync-env [options] [keys...]          Resolve keys (or --for <app>) through
                                        the active backend and write a .env —
                                        the deploy-time replacement for
                                        hand-editing .env over SSH
  cache clear                           Remove the local encrypted copy of the
                                        vault (see `wf secret cache --help`)
  doctor [options]                      Compare the registry against the vault
                                        in BOTH directions and exit non-zero on
                                        drift — a registered key the backend
                                        cannot resolve, or a vault secret
                                        nothing maps to
  scope-check [options]                 Check each CI job's keys against
                                        their scopes (open jobs read no
                                        ci-deploy key) — JSON, never a value
  parity [options]                      Per CI scope, the keys the vault holds
                                        against the registry and a keyed hash
                                        of each value, to compare two runner
                                        kinds — JSON, never a value
  push-notes [options]                  Sync each secret's registry description
                                        into its Bitwarden note (one-way:
                                        registry -> vault). DRY RUN unless
                                        --apply
  create [options] <KEY> <PROJECT_ID>   Create a vault secret, using its
                                        registry description as the note. DRY
                                        RUN unless --apply
  update [options] <KEY>                Replace an EXISTING vault secret's value
                                        in place, keeping its key, note, project
                                        and id — the rotation path, and how a
                                        `--placeholder` secret gets filled in.
                                        Refuses if the key is not already in the
                                        vault. DRY RUN unless --apply
  rename [options] <FROM_KEY> <TO_KEY>  Rename a vault secret in place,
                                        preserving its id (so UUID-addressed
                                        consumers keep working). DRY RUN unless
                                        --apply
  move [options] <KEY> <PROJECT_ID>     Move a vault secret to another project
                                        in place, keeping its key, value, note
                                        and id. DRY RUN unless --apply
  registry                              Secrets registry maintenance
  login                                 Capture a tool's own login (claude, codex,
                                        ...): `wf secret login <tool>`
  help [command]                        display help for command
```

</details>

### `wf secret cache`

Remove the local copy of the vault. With the Bitwarden backend a wf command reads the vault once and keeps an encrypted copy (XChaCha20-Poly1305, keyed from the access token, 0600) for an hour (WF_SECRETS_CACHE_TTL_SEC), so later commands do not wait on it and a vault outage does not stop them: a copy up to a week old stands in with a warning. The write-capable source is never kept.

Usage: `wf secret cache [options] clear`

Examples:

```sh
wf secret cache clear
```

<details><summary>Full help</summary>

```text
Usage: wf secret cache [options] clear

Remove the local copy of the vault. With the Bitwarden backend a wf command reads
the vault once and keeps an encrypted copy (XChaCha20-Poly1305, keyed from the
access token, 0600) for an hour (WF_SECRETS_CACHE_TTL_SEC), so later commands do
not wait on it and a vault outage does not stop them: a copy up to a week old
stands in with a warning. The write-capable source is never kept.

Environment: WF_SECRETS_CACHE=off skips it, =refresh fetches and rewrites it;
WF_CACHE_DIR moves it (default ~/.cache/wf). `wf secret sync-env` always
refreshes, and a successful vault write clears it.

Options:
  -h, --help  display help for command
```

</details>

### `wf secret create`

Create a vault secret, using its registry description as the note. DRY RUN unless --apply

Usage: `wf secret create [options] <KEY> <PROJECT_ID>`

| Option | Meaning |
|---|---|
| `--value <value>` | the secret value |
| `--value-stdin` | read the value from stdin — the ONLY way to get a secret into the vault without it appearing in argv (visible to `ps`) or in shell history |
| `--value-from <KEY>` | copy the value from an existing registered secret (never printed) |
| `--placeholder` | create with a PLACEHOLDER_FILL_ME value, to be filled in by hand later |
| `--apply` | actually create it (default is a dry run that changes nothing) |

Examples:

```sh
wf secret create
```

<details><summary>Full help</summary>

```text
Usage: wf secret create [options] <KEY> <PROJECT_ID>

Create a vault secret, using its registry description as the note. DRY RUN
unless --apply

Options:
  --value <value>     the secret value
  --value-stdin       read the value from stdin — the ONLY way to get a secret
                      into the vault without it appearing in argv (visible to
                      `ps`) or in shell history
  --value-from <KEY>  copy the value from an existing registered secret (never
                      printed)
  --placeholder       create with a PLACEHOLDER_FILL_ME value, to be filled in
                      by hand later
  --apply             actually create it (default is a dry run that changes
                      nothing)
  -h, --help          display help for command
```

</details>

### `wf secret delete`

Remove secret — macOS backend only

Usage: `wf secret delete [options] <KEY>`

Examples:

```sh
wf secret delete OLD_UNUSED_TOKEN
```

<details><summary>Full help</summary>

```text
Usage: wf secret delete [options] <KEY>

Remove secret — macOS backend only

Options:
  -h, --help  display help for command
```

</details>

### `wf secret doctor`

Compare the registry against the vault in BOTH directions and exit non-zero on drift — a registered key the backend cannot resolve, or a vault secret nothing maps to

Usage: `wf secret doctor [options]`

| Option | Meaning |
|---|---|
| `--backend <name>` | override the active backend (macos &#124; bitwarden) |
| `--if-available` | exit 0 with a skip notice when no vault credential is present at all, instead of failing — for `arch`/CI, where a contributor or a fork has no vault access and a hard failure would get the check deleted |

Examples:

```sh
wf secret doctor --backend bitwarden --if-available
wf secret doctor --backend bitwarden
```

<details><summary>Full help</summary>

```text
Usage: wf secret doctor [options]

Compare the registry against the vault in BOTH directions and exit non-zero on
drift — a registered key the backend cannot resolve, or a vault secret nothing
maps to

Options:
  --backend <name>  override the active backend (macos | bitwarden)
  --if-available    exit 0 with a skip notice when no vault credential is
                    present at all, instead of failing — for `arch`/CI, where a
                    contributor or a fork has no vault access and a hard failure
                    would get the check deleted
  -h, --help        display help for command
```

</details>


### `wf secret verify`

Check that every secret an app's deploy carries is usable, not only present: hygiene for all (no stray whitespace or newline), then the shape and the read-only provider call that the secret's own registry entry names (`check` in clis/secrets/src/data/registry.json; a secret with none carries an `uncheckable` reason). Prints JSON (app, ok, a finding per key with its status and the checks that ran) and never a value. Exits 1 when any secret is missing or unusable; a provider that does not answer is reported as "unchecked", never as a failure.

Usage: `wf secret verify [options]`

| Option | Meaning |
|---|---|
| `--for <app>` | the app whose secrets to check (a `sync-env --for` group) |
| `--scope <scope>` | check the keys of one CI scope (ci-open &#124; ci-deploy) against the token's vault project instead of an app's group |
| `--backend <name>` | override the active backend (macos &#124; bitwarden) |

Examples:

```sh
wf secret verify --for leetcode-backend
wf secret verify --for agent-shell --backend bitwarden
```

<details><summary>Full help</summary>

```text
Usage: wf secret verify [options]

Check that every secret an app's deploy carries is usable, not only present:
hygiene for all (no stray whitespace or newline), then the shape and the
read-only provider call that the secret's own registry entry names (`check` in
clis/secrets/src/data/registry.json; a secret with none carries an
`uncheckable` reason). Prints JSON (app, ok, a finding per key with its status
and the checks that ran) and never a value. Exits 1 when any secret is missing or unusable; a provider that does not
answer is reported as "unchecked", never as a failure.

With --scope the check is of a CI vault project instead of an app: every
registry key whose `scope` is the one named must be in the project the token
reads (`missing`), and the project must list no key of another scope
(`exposed`, with that key's scope). Prints JSON (scope, ok, keys, missing,
exposed) with names only. Exits 1 on any missing or exposed key.

Options:
  --for <app>       the app whose secrets to check (a `sync-env --for` group)
  --scope <scope>   check a CI vault project instead: ci-open | ci-deploy
  --backend <name>  override the active backend (macos | bitwarden)
  -h, --help        display help for command
```

</details>

### `wf secret env`

Print KEY=value lines for all available secrets (eval-safe)

Usage: `wf secret env [options]`

| Option | Meaning |
|---|---|
| `--backend <name>` | macos &#124; bitwarden |
| `--keys <list>` | only these registry keys (or env var names), comma-separated |

Examples:

```sh
wf secret env --backend bitwarden
wf secret env prints every resolved secret value at once — redirected to a file, piped into a dump or network command, or fo
```

<details><summary>Full help</summary>

```text
Usage: wf secret env [options]

Print KEY=value lines for all available secrets (eval-safe)

Options:
  --backend <name>  macos | bitwarden
  --keys <list>     only these registry keys (or env var names), comma-separated
  -h, --help        display help for command
```

</details>

### `wf secret get`

Print raw value (exits 1 if missing)

Usage: `wf secret get [options] <KEY>`

| Option | Meaning |
|---|---|
| `--backend <name>` | macos &#124; bitwarden |

Examples:

```sh
wf secret get VNC_AUTH_TRIGGER_SECRET --backend bitwarden
wf secret get CHAT_AGENT_API_SECRET
```

<details><summary>Full help</summary>

```text
Usage: wf secret get [options] <KEY>

Print raw value (exits 1 if missing)

Options:
  --backend <name>  macos | bitwarden
  -h, --help        display help for command
```

</details>

### `wf secret list`

Show all registered secrets and their sources

Usage: `wf secret list [options]`

| Option | Meaning |
|---|---|
| `--backend <name>` | macos &#124; bitwarden |

Examples:

```sh
wf secret list is used, then it does not fire even alongside a dump command
wf secret list --backend bitwarden
```

<details><summary>Full help</summary>

```text
Usage: wf secret list [options]

Show all registered secrets and their sources

Options:
  --backend <name>  macos | bitwarden
  -h, --help        display help for command
```

</details>

### `wf secret login`

Capture interactive tool logins (claude, codex, gcloud, ...): `wf secret login <tool>` starts the login in the shared VNC desktop's browser, prints the link to open on any device (a phone is enough) and waits until the login finished. The login stays on the VM and agent-shell gets its credential; `--copy` also writes the files the tool's config declares copyable to this machine's home. A tool whose config names a vault secret (gcloud) also has it set when the login finishes, and emptied by `--clear` (both need the vault write token).

Usage: `wf secret login [options] [command]`

| Option | Meaning |
|---|---|
| `--copy` | also copy the finished login's credential files to this machine |
| `--clear` | log the tool out first (its credential files here and in the VM, a revoke where the tool has one, the login browser's cookies), so the login starts from nothing |

Examples:

```sh
wf secret login claude
wf secret login gcloud --clear --copy
```

<details><summary>Full help</summary>

```text
Usage: wf secret login [options] [command]

Capture interactive tool logins (claude, codex, gcloud, ...): `wf secret login
<tool>` starts the login in the shared VNC desktop's browser, prints the link to
open on any device (a phone is enough) and waits until the login finished. The
login stays on the VM and agent-shell gets its credential; `--copy` also writes
the files the tool's config declares copyable to this machine's home. A tool
whose config names a vault secret (gcloud) also has it set when the login
finishes, and emptied by --clear (both need the vault write token).

Options:
  --copy      also copy the finished login's credential files to this machine
  --clear     log the tool out first (its credential files here and in the VM,
              a revoke where the tool has one, the login browser's cookies), so
              the login starts from nothing
  -h, --help  display help for command

Commands:
  list        List tools with a known auth descriptor
  capture     Run an ARBITRARY interactive login on the shared VNC desktop from a
              JSON spec and capture the credential out of the browser
  done        Signal that an interactive login is finished (what actually
              captures a browser-only flow's credential)
  launch-url  Navigate the shared VNC desktop's browser to an allowlisted URL
  listener    Run the auth-session listener (inside the shared desktop)
  help [command]  display help for command
```

</details>

#### `wf secret login capture`

Run an ARBITRARY interactive login on the shared VNC desktop from a JSON spec: open a URL, let a human finish it on their phone, then capture the credential out of the browser and deliver it wherever the spec says. Nothing about the spec's tool needs to exist in this codebase.

Usage: `wf secret login capture [options]`

| Option | Meaning |
|---|---|
| `--spec <path>` | Path to a JSON spec file |
| `--spec-json <json>` | The JSON spec inline |

Examples:

```sh
wf secret login capture --spec <spec.json>
```

<details><summary>Full help</summary>

```text
Usage: wf secret login capture [options]

Run an ARBITRARY interactive login on the shared VNC desktop from a JSON spec:
open a URL, let a human finish it on their phone, then capture the credential
out of the browser and deliver it wherever the spec says. Nothing about the
spec's tool needs to exist in this codebase.

Options:
  --spec <path>       Path to a JSON spec file
  --spec-json <json>  The JSON spec inline
  -h, --help          display help for command
```

</details>

#### `wf secret login done`

Signal that an interactive login is finished. This is what actually captures the credential for a browser-only flow -- until it runs, the credential exists nowhere but the browser.

Usage: `wf secret login done [options] <sessionId>`

Examples:

```sh
wf secret login done {}\n
```

<details><summary>Full help</summary>

```text
Usage: wf secret login done [options] <sessionId>

Signal that an interactive login is finished. This is what actually captures the
credential for a browser-only flow -- until it runs, the credential exists
nowhere but the browser.

Options:
  -h, --help  display help for command
```

</details>

#### `wf secret login launch-url`

Navigate the shared VNC desktop's browser to <url> (e.g. a magic-link URL copied out of an inbox the desktop has no access to). The origin must be allowlisted (claude.ai, console.anthropic.com, auth.openai.com, chatgpt.com, accounts.google.com) -- the listener rejects anything else before it ever reaches the desktop.

Usage: `wf secret login launch-url [options] <url>`

Examples:

```sh
wf secret login launch-url
```

<details><summary>Full help</summary>

```text
Usage: wf secret login launch-url [options] <url>

Navigate the shared VNC desktop's browser to <url> (e.g. a magic-link URL copied
out of an inbox the desktop has no access to). The origin must be allowlisted
(claude.ai, console.anthropic.com, auth.openai.com, chatgpt.com,
accounts.google.com) -- the listener rejects anything else before it ever
reaches the desktop.

Options:
  -h, --help  display help for command
```

</details>

#### `wf secret login list`

List tools with a known auth descriptor

Usage: `wf secret login list [options]`

Examples:

```sh
wf secret login list
```

<details><summary>Full help</summary>

```text
Usage: wf secret login list [options]

List tools with a known auth descriptor

Options:
  -h, --help  display help for command
```

</details>

#### `wf secret login listener`

Run the auth-session listener (inside the shared desktop container): mints the tokened desktop link, keeps the browser presenting as a phone, captures browser-only credentials on `done` and relays credential files back to agent-shell. Configured by the VNC_AUTH_* environment.

Usage: `wf secret login listener [options]`

Examples:

```sh
wf secret login listener
```

<details><summary>Full help</summary>

```text
Usage: wf secret login listener [options]

Run the auth-session listener (inside the shared desktop container): mints the
tokened desktop link, keeps the browser presenting as a phone, captures
browser-only credentials on `done` and relays credential files back to
agent-shell. Configured by the VNC_AUTH_* environment.

Options:
  -h, --help  display help for command
```

</details>

### `wf secret move`

Move a vault secret to another project in place, keeping its key, value, note and id — how a key reaches the machine accounts that read that project (the deploy read token reads mobile-learning-deploy-secrets only). DRY RUN unless --apply

Usage: `wf secret move [options] <KEY> <PROJECT_ID>`

| Option | Meaning |
|---|---|
| `--apply` | actually move (default is a dry run that changes nothing) |

Examples:

```sh
wf secret move CH_BACKEND_HOST 65501031-2ee6-4d57-9e45-b48e0175233f --apply
```

<details><summary>Full help</summary>

```text
Usage: wf secret move [options] <KEY> <PROJECT_ID>

Move a vault secret to another project in place, keeping its key, value, note
and id — how a key reaches the machine accounts that read that project (the
deploy read token reads mobile-learning-deploy-secrets only). DRY RUN unless
--apply

Options:
  --apply     actually move (default is a dry run that changes nothing)
  -h, --help  display help for command
```

</details>

### `wf secret push-notes`

Sync each secret's registry description into its Bitwarden note (one-way: registry -> vault). DRY RUN unless --apply

Usage: `wf secret push-notes [options]`

| Option | Meaning |
|---|---|
| `--apply` | actually write the notes (default is a dry run that changes nothing) |
| `--verbose` | also list the registered-but-absent-from-vault keys individually (they are only counted by default) |

Examples:

```sh
wf secret push-notes --apply
```

<details><summary>Full help</summary>

```text
Usage: wf secret push-notes [options]

Sync each secret's registry description into its Bitwarden note (one-way:
registry -> vault). DRY RUN unless --apply

Options:
  --apply     actually write the notes (default is a dry run that changes
              nothing)
  --verbose   also list the registered-but-absent-from-vault keys individually
              (they are only counted by default)
  -h, --help  display help for command
```

</details>

### `wf secret registry`

Secrets registry maintenance

Usage: `wf secret registry [options] [command]`

Examples:

```sh
wf secret registry sync
```

<details><summary>Full help</summary>

```text
Usage: wf secret registry [options] [command]

Secrets registry maintenance

Options:
  -h, --help      display help for command

Commands:
  sync            Rebuild wf after editing registry.json (the registry is
                  compiled into it), then re-run the same drift check the
                  pre-commit hook runs
  help [command]  display help for command
```

</details>

#### `wf secret registry sync`

Rebuild wf after editing registry.json (the registry is compiled into it), then re-run the same drift check the pre-commit hook runs

Usage: `wf secret registry sync [options]`

<details><summary>Full help</summary>

```text
Usage: wf secret registry sync [options]

Rebuild wf after editing registry.json (the registry is compiled into it), then
re-run the same drift check the pre-commit hook runs

Options:
  -h, --help  display help for command
```

</details>

### `wf secret rename`

Rename a vault secret in place, preserving its id (so UUID-addressed consumers keep working). DRY RUN unless --apply

Usage: `wf secret rename [options] <FROM_KEY> <TO_KEY>`

| Option | Meaning |
|---|---|
| `--additive` | keep the old key and copy the value to the new one — the zero-window path for a secret a running deploy resolves by name |
| `--apply` | actually rename (default is a dry run that changes nothing) |

Examples:

```sh
wf secret rename --additive
```

<details><summary>Full help</summary>

```text
Usage: wf secret rename [options] <FROM_KEY> <TO_KEY>

Rename a vault secret in place, preserving its id (so UUID-addressed consumers
keep working). DRY RUN unless --apply

Options:
  --additive  keep the old key and copy the value to the new one — the
              zero-window path for a secret a running deploy resolves by name
  --apply     actually rename (default is a dry run that changes nothing)
  -h, --help  display help for command
```

</details>

### `wf secret parity`

For each CI scope (ci-open, ci-deploy), list the registry keys, the keys the scope's vault project holds, the ones it lacks, a hash of each name list and a keyed hash (HMAC-SHA256 of key name and value under the nonce) of every value held. Run it on two runner kinds with the same `--nonce` and diff the JSON. Prints JSON and never a value. Exits 1 when a scope lacks a registry key, except a key that only deploy groups list as optional (listed as missingOptional).

Usage: `wf secret parity [options]`

| Option | Meaning |
|---|---|
| `--json` | accepted; the output is always JSON |
| `--nonce <text>` | the key to hash with, at least 16 characters (default: a random one for this run, printed in the output) |
| `--backend <name>` | override the active backend (macos \| bitwarden) |

Examples:

```sh
wf secret parity --json --nonce "$RUN_NONCE" > parity-$RUNNER_NAME.json
```

<details><summary>Full help</summary>

```text
Usage: wf secret parity [options]

For each CI scope (ci-open, ci-deploy), list the registry keys, the keys the
scope's vault project holds, the ones it lacks, a hash of each name list, and a
keyed hash (HMAC-SHA256 of the key name and value under the nonce) of every
value held. Run it on two runner kinds with the same --nonce and compare the
JSON: equal output means the same keys and the same values, and a stale or
divergent value shows as a different hash. Prints JSON and never a value.
Exits 1 when a scope lacks a registry key, except a key that only deploy groups list as optional (listed as missingOptional). The nonce is printed, so use it
for high-entropy values only (tokens, URLs with a signature).

Options:
  --json            accepted; the output is always JSON
  --nonce <text>    the key to hash with, at least 16 characters (default: a
                    random one for this run, printed in the output)
  --backend <name>  override the active backend (macos | bitwarden)
  -h, --help        display help for command
```

</details>

### `wf secret scope-check`

Check the keys each CI job asks for against the scope of each key, from the workflow files and clis/secrets/src/data/job-scopes.json. An open job may ask for ci-open and public keys; a deploy job also for ci-deploy; no job for a vm key. Prints JSON (ok, a line per job, all violations) and never a value. Exits 1 on any violation.

Usage: `wf secret scope-check [options]`

| Option | Meaning |
|---|---|
| `--json` | accepted; the output is always JSON |
| `--workflows <dir>` | the workflow files to read (default .github/workflows) |
| `--jobs <file>` | the job classes to use (default job-scopes.json) |

Examples:

```sh
wf secret scope-check --json
```

<details><summary>Full help</summary>

```text
Usage: wf secret scope-check [options]

Check the keys each CI job asks for against the scope of each key, from the
workflow files (the groups its `wf secret ... --for <group>` lines resolve and
the registry keys they name) and clis/secrets/src/data/job-scopes.json (each
job's class, open or deploy, and the keys no step names, such as a deploy
leg's group). An open job may ask for ci-open and public keys; a deploy job
also for ci-deploy; no job for a vm key. Prints JSON (ok, a line per job with
its class, the keys it asks for with their scope, and its violations, then
all violations) and never a value. Exits 1 on any violation.

Options:
  --json              accepted; the output is always JSON
  --workflows <dir>   the workflow files to read (default .github/workflows)
  --jobs <file>       the job classes to use (default job-scopes.json)
  -h, --help          display help for command
```

</details>


### `wf secret set`

Store secret (reads from stdin if value omitted) — macOS backend only

Usage: `wf secret set [options] <KEY> [value]`

Examples:

```sh
wf secret set BWS_ADMIN_ACCESS_TOKEN
```

<details><summary>Full help</summary>

```text
Usage: wf secret set [options] <KEY> [value]

Store secret (reads from stdin if value omitted) — macOS backend only

Options:
  -h, --help  display help for command
```

</details>

### `wf secret sync-env`

Resolve keys (or --for <app>) through the active backend and write a .env — the deploy-time replacement for hand-editing .env over SSH

Usage: `wf secret sync-env [options] [keys...]`

| Option | Meaning |
|---|---|
| `--for <app>` | named key group, e.g. leetcode-backend, remote-chat-agent, leetcode-auth-cookie-capture, vm-gateway, vnc-auth-session, vm-ops-api, agent-shell, vm-jobs, reporting-service, oracle-terraform |
| `--backend <name>` | macos &#124; bitwarden |
| `--out <file>` | write to this file instead of stdout |
| `--base <file>` | non-secret base to merge under the resolved secrets (default: .env.defaults in cwd if present) |
| `--reveal` | without --out, print REAL secret values to stdout instead of a redacted preview -- only at a real terminal you trust, never in a script or agent session (see docs/solutions/security/sync-env-stdout-redaction.md) |

Examples:

```sh
wf secret sync-env --for vnc-auth-session
wf secret sync-env --base .env.defaults
wf secret sync-env --for vnc-auth-session --backend bitwarden
```

<details><summary>Full help</summary>

```text
Usage: wf secret sync-env [options] [keys...]

Resolve keys (or --for <app>) through the active backend and write a .env — the
deploy-time replacement for hand-editing .env over SSH

Options:
  --for <app>       named key group, e.g. leetcode-backend, remote-chat-agent,
                    leetcode-auth-cookie-capture, vm-gateway, vnc-auth-session,
                    vm-ops-api, agent-shell, vm-jobs, reporting-service,
                    oracle-terraform
  --backend <name>  macos | bitwarden
  --out <file>      write to this file instead of stdout
  --base <file>     non-secret base to merge under the resolved secrets
                    (default: .env.defaults in cwd if present)
  --reveal          without --out, print REAL secret values to stdout instead of
                    a redacted preview -- only at a real terminal you trust,
                    never in a script or agent session (see
                    docs/solutions/security/sync-env-stdout-redaction.md)
  -h, --help        display help for command
```

</details>

### `wf secret update`

Replace an EXISTING vault secret's value in place, keeping its key, note, project and id — the rotation path, and how a `--placeholder` secret gets filled in. Refuses if the key is not already in the vault. DRY RUN unless --apply

Usage: `wf secret update [options] <KEY>`

| Option | Meaning |
|---|---|
| `--value <value>` | the new value |
| `--value-stdin` | read the new value from stdin — keeps it out of argv (visible to `ps`) and shell history |
| `--apply` | actually write it (default is a dry run that changes nothing) |

Examples:

```sh
wf secret update BETTERSTACK_API_TOKEN --value-stdin --apply
wf secret update --apply
```

<details><summary>Full help</summary>

```text
Usage: wf secret update [options] <KEY>

Replace an EXISTING vault secret's value in place, keeping its key, note,
project and id — the rotation path, and how a `--placeholder` secret gets filled
in. Refuses if the key is not already in the vault. DRY RUN unless --apply

Options:
  --value <value>  the new value
  --value-stdin    read the new value from stdin — keeps it out of argv (visible
                   to `ps`) and shell history
  --apply          actually write it (default is a dry run that changes nothing)
  -h, --help       display help for command
```

</details>

## ship

### `wf ship`

Redeploy a service through vm-ops-api at a tag CI pushed. Nothing here builds an image: CI builds and pushes every image of a push to dev on the runner (`wf ship build <app> --tag <tag>`, see `wf ship build --help`), and `wf ship <service> --ref <sha> --redeploy-only --tag sha-<8>` redeploys what it pushed. A hand build is docker buildx in agent-shell's own docker, never into a production tag.

Usage: `wf ship [options] [service...]`

| Option | Meaning |
|---|---|
| `--ref <ref>` | Branch or commit to provision config from (the commit a --wait-serving waits for) |
| `--tag <tag>` | Docker tag CI pushed (default: latest) |
| `--redeploy-only` | Pull+restart the service at --tag (required for a service with an image: wf ship never builds) |
| `--smoke` | With --redeploy-only: first run the pushed tag as a throwaway container and probe it (nothing is swapped on a failure); CI's deploy legs use it after `wf ship build`. vm-ops-api always proves its own image first |
| `--wipe-state` | Also wipe the tailscale-state volume |
| `--recreate-all` | Recreate every container of the service, sidecars included |
| `--changed` | Print which services a push must deploy, each with why, and whether it publishes an EAS Update: {"legs":[{"app","why":[...]}],"easUpdate":{"publish","why":[...]}}. A service is planned when the hash of its images and compose project differs between the commit it runs and HEAD (or that commit is unknown or not in the checkout). The update is published when the app's package graph (and the lockfile entries it reaches) changed since the commit of the branch's latest-dev.json pointer, when a tailnet name that update baked in has moved, or when that commit cannot be read or the changed packages cannot be listed; a rotated backend token is not seen, publish it with --force (--force: all legs and the update; --also <a,b>: those services too, changed or not; --since <sha>: compare with that commit instead of what each service runs and the pointer names, asking vm-ops-api and the bucket nothing, so no tailnet name is checked either; --github-output: append legs= and eas_update= to $GITHUB_OUTPUT) |
| `--status` | Deployer state: vm-ops-api /health and the container status of each service |

Examples:

```sh
wf ship leetcode-backend --ref dev --redeploy-only --tag sha-1a2b3c4d
```

<details><summary>Full help</summary>

```text
Usage: wf ship [options] [service...]

Redeploy a service through vm-ops-api at a tag CI pushed. Nothing here builds an image: CI builds and pushes every image of a push to dev on the runner (`wf ship build <app> --tag <tag>`, see `wf ship build --help`), and `wf ship <service> --ref <sha> --redeploy-only --tag sha-<8>` redeploys what it pushed. A hand build is docker buildx in agent-shell's own docker, never into a production tag.
`wf ship ci-runner --ref <ref>` mints a one-hour runner registration token with VM_PROVISION_GITHUB_TOKEN and redeploys the runner with it as RUNNER_TOKEN, plus a one-hour single-use tag:ci-self-hosted Tailscale key as TS_CI_RUNNER_AUTH_KEY so the runner node joins with its own tag (when no key can be minted it prints one line and the runner keeps joining with the shared key); the PAT and both values go nowhere else and are never printed. With --wipe-state the runner's volumes (its tailnet identity and registration) are wiped first, which moves an existing node onto the new tag; a key already in TS_CI_RUNNER_AUTH_KEY in the environment is sent as is instead of minted (mint it where the Tailscale credentials are readable).
A redeploy prints the seconds of each of its steps (`phase <name>: <n> s`): with --smoke, a smoke test (the pushed image run once as a throwaway container on a spare port beside the live one; a failure prints that container's log and swaps nothing) and a tailnet probe; the pull and swap; and, with --wait-serving, one bounded /health check. An app with no image (ci-runner) redeploys at latest.
`wf ship vm-ops-api --ref <commit> --redeploy-only --tag <tag>` (--ref must name a commit) asks POST /redeploy?ref=<sha>&tag=<tag> to prove the image and swap vm-ops-api itself in; a swap that fails is recorded in /redeploy/status and --wait-serving reports it at once.
--wait-serving: the deploy is done only when the service serves the --ref commit (its /health), within 2 minutes; a service with no /health (vnc-auth-session, leetcode-auth-cookie-capture) is confirmed by the sha tag of its running image. CI's deploy legs use it.

Options:
  --ref <ref>      Branch or commit to provision config from (the commit a
                   --wait-serving waits for)
  --tag <tag>      Docker tag CI pushed (default: latest)
  --redeploy-only  Pull+restart the service at --tag (required for a service
                   with an image: wf ship never builds)
  --smoke          With --redeploy-only: first run the pushed tag as a throwaway
                   container and probe it (nothing is swapped on a failure); CI's
                   deploy legs use it after `wf ship build`. vm-ops-api always
                   proves its own image first
  --promote-latest Point the named images' latest at --tag, in a VM job that must
                   end 0: `wf ship --promote-latest vm-ops-api-rs watchdog --tag sha-<7>`.
                   It is an exec-script job, admin port only, so it fails from a
                   CI job (WF_VM_CI_PORT set); CI moves `latest` with
                   `wf ship promote <app> --tag <tag>`, which needs no route.
  --wipe-state     Also wipe the tailscale-state volume
  --recreate-all   Recreate every container of the service, sidecars included
  --changed        Print which services a push must deploy, each with why, and
                   whether it publishes an EAS Update: {"legs":[{"app","why":[...]}],
                   "easUpdate":{"publish","why":[...]}}. A service is planned when the hash of its
                   images and compose project differs between the commit it runs and HEAD
                   (or that commit is unknown or not in the checkout). The update is published when the
                   app's package graph (and the lockfile entries it reaches) changed since
                   the commit of the branch's latest-dev.json pointer, when a tailnet name
                   that update baked in has moved, or when that commit cannot be read or
                   the changed packages cannot be listed; a rotated backend token is not
                   seen, publish it with --force (--force: all legs and the update;
                   --since <sha>: compare with that commit instead of what
                   each service runs and the pointer names, asking vm-ops-api and the
                   bucket nothing, so no tailnet name is checked either; --github-output:
                   append legs= and eas_update= to $GITHUB_OUTPUT)
  --status         Deployer state: vm-ops-api /health and the container status
                   of each service
  -h, --help       display help for command

Commands:
  eas [options]    Publish the app to Expo as an EAS Update
  retire <service> Take down the compose project of a removed service
  worker [--sha <commit>] [--print]
                   Build that commit's wf on the VM and install it for the
                   test-queue worker (a dev deploy never updates it); JSON
                   with ok, sha, seconds, sourceChecksum
```

</details>

### `wf ship eas`

Publish the app to Expo as an EAS Update (frontend/mla-tamagui's publish:eas script). Needs EXPO_TOKEN from the vault. EXPO_PUBLIC_* variables in the environment are baked into the published bundle.

Usage: `wf ship eas [options]`

| Option | Meaning |
|---|---|
| `--branch <branch>` | EAS branch to publish to (default: the current git branch) |
| `--initial-route <route>` | Route the update opens on (default: /) |
| `--message <message>` | Update message (default: the last commit subject) |
| `--platform <platform>` | all \| ios \| android: one update group for those platforms (default: the script's own, ios) |
| `--pointer` | Also store the summary as the branch's latest update, which the e2e tests load (CI's publish only: they refuse an update without the lab flag, backend hosts and token CI bakes in, or whose baked tailnet names have moved) |
| `--latest` | Publish nothing: print the update the branch pointer names (CI's last publish; --branch defaults to dev) as {branch, commit, group, platforms, link}; exit 1 with {ok:false, error, next} when none is stored |
| `--frontend <name>` | Frontend package to publish (default: mla-tamagui) |

Examples:

```sh
wf ship eas --initial-route /your-route
wf ship eas --latest
wf ship eas --platform all --branch dev --pointer --message "$(git log -1 --format=%s "$GITHUB_SHA")"
```

<details><summary>Full help</summary>

```text
Usage: wf ship eas [options]

Publish the app to Expo as an EAS Update (frontend/mla-tamagui's publish:eas
script). Needs EXPO_TOKEN from the vault. EXPO_PUBLIC_* variables in the
environment are baked into the published bundle.

Options:
  --branch <branch>          EAS branch to publish to (default: the current git
                             branch)
  --initial-route <route>    Route the update opens on (default: /)
  --message <message>        Update message (default: the last commit subject)
  --platform <platform>      all | ios | android: one update group for those
                             platforms (default: the script's own, ios)
  --pointer                  Also store the summary as the branch's latest
                             update, which the e2e tests load (CI's publish
                             only: they refuse an update without the lab flag,
                             backend hosts and token CI bakes in, or whose baked
                             tailnet names have moved)
  --latest                   Publish nothing: print the update the branch pointer
                             names (CI's last publish; --branch defaults to dev)
                             as {branch, commit, group, platforms, link}; exit 1
                             with {ok:false, error, next} when none is stored
  --frontend <name>          Frontend package to publish (default: mla-tamagui)
  -h, --help                 display help for command

Output: one JSON line on stdout {published, branch, initialRoute, commit, group,
platform, platforms[], env[], hosts[], memory {peakRssMb, vm}, timing {exportS,
uploadS, servedS}, link, maps {uploaded, base, files[] | error}, served
{platform, runtimeVersion, url, status, s | error}, pointers {summary, latest |
error}}. env holds the names (never the values) of the EXPO_PUBLIC_* variables
the bundle baked in, hosts the hosts their URLs name. peakRssMb is the largest
process the publish ran; vm is the VM's load and MemAvailable sampled during it
when WF_VM_SAMPLE_SECS is set. servedS is the seconds after the upload until
u.expo.dev serves the group's manifest to a device of that platform (android
when the group has it; WF_EAS_UPDATES_URL overrides the host, and
WF_EAS_SERVED_TIMEOUT_S the 120 s it waits). The line is stored in the
verification bucket as verification/eas-updates/<group>/summary.json and, with
--pointer, as verification/eas-updates/latest-<branch>.json.
Exit 1 when the publish fails (the tail of its output on stderr) or, with
--pointer, when latest-<branch>.json was not stored (it still names an earlier
update); 2 on a bad option.
```

</details>

### `wf ship build`

Build the images of an app and its sidecar images with the docker CLI against whatever DOCKER_HOST names, and push them to docker.io/nuesslerm

Usage: `wf ship build [options] <app>`

| Option | Meaning |
|---|---|
| `--tag <tag>` | Docker tag to push (required) |
| `--ref <ref>` | Commit of this checkout to hash and label the images with (default HEAD) |
| `--platform <p>` | Target platform (default linux/arm64, what the VM runs) |
| `--cache-push` | Also export the layer cache to the registry (docker.io/nuesslerm/<image>:buildcache, mode=max); every build reads it. CI sets it on a push to dev only |
| `--images` | Print {"app","images":[...]} (empty for an app with no image) and exit: no credential, no docker call |

<details><summary>Full help</summary>

```text
Usage: wf ship build [options] <app>

Build the images of an app, and the sidecar images its compose pulls on the same
tag, with the docker CLI against whatever DOCKER_HOST names, and push them to
docker.io/nuesslerm. In CI that is the builder sidecar of the VM runner, or the
local daemon of a hosted runner (the build-image action). It never touches the VM
or vm-ops-api. An image whose input hash (nx-<hash>) is already in the registry is
re-tagged for the commit, never rebuilt. Prints one JSON line:
{"app","tag","commit","images":[{"image","ref","reused","pushed"}]}.
DOCKERHUB_TOKEN (and DOCKERHUB_USERNAME, default nuesslerm) are read from the
environment and given to `docker login` on stdin; they are never printed.

Options:
  --tag <tag>        Docker tag to push (required)
  --ref <ref>        Commit of this checkout to hash and label the images with
                     (default HEAD)
  --platform <p>     Target platform (default linux/arm64, what the VM runs)
  --cache-push       Also export the layer cache to the registry
                     (docker.io/nuesslerm/<image>:buildcache, mode=max); every
                     build reads it. CI sets it on a push to dev only
  --no-push          Build every image from scratch and push nothing: no tag,
                     no cache export, no re-tag of an image whose hash the
                     registry holds. The GitHub-hosted twin of the CI uses it
                     to time the builds without shipping (its images are x64)
  --fresh            Rebuild every image with no layer cache and no reuse of an
                     image of the same hash, and push it as usual (a commit tag
                     `[cache:no]` sets it in CI)
  --images           Print {"app","images":[...]} (empty for an app with no image)
                     and exit: no credential, no docker call
  -h, --help         display help for command
```

</details>

### `wf ship promote`

Point `latest` of every image of an app at a tag, registry to registry, with the docker CLI against whatever DOCKER_HOST names

Usage: `wf ship promote [options] <app>`

| Option | Meaning |
|---|---|
| `--tag <tag>` | Tag whose manifest `latest` gets (required), e.g. sha-<8> |

<details><summary>Full help</summary>

```
Usage: wf ship promote [options] <app>

Point `latest` of every image of an app (the same list `wf ship build` pushes) at
--tag, registry to registry: `docker buildx imagetools create -t
docker.io/nuesslerm/<image>:latest docker.io/nuesslerm/<image>:<tag>`, against
whatever DOCKER_HOST names. It never touches the VM or vm-ops-api, so it runs from
a CI job on the CI port. CI calls it after a deploy leg served the commit, for
vm-ops-api (vm-ops-api-rs and watchdog), whose `latest` a recreated VM pulls.
Prints one JSON line: {"app","tag","images":[{"image","latest","from"}]}.
DOCKERHUB_TOKEN (and DOCKERHUB_USERNAME, default nuesslerm) are read from the
environment and given to `docker login` on stdin; they are never printed.

Options:
  --tag <tag>        Tag whose manifest `latest` gets (required), e.g. sha-<8>
  -h, --help         display help for command
```

</details>

### `wf ship retire`

Take down the compose project of a service that was removed from the service list (refused for a live service)

Usage: `wf ship retire [options] <service>`

Examples:

```sh
wf ship retire vm-gateway
```

<details><summary>Full help</summary>

```text
Usage: wf ship retire [options] <service>

Take down the compose project of a service that was removed from the service
list (refused for a live service)

Options:
  -h, --help  display help for command
```

</details>

## test

### `wf t3`

A server that speaks the T3 Code client protocol (orchestration protocol 2, T3

Usage: `wf t3 [options] [command]`

<details><summary>Full help</summary>

```text
Usage: wf t3 [options] [command]

A server that speaks the T3 Code client protocol (orchestration protocol 2, T3
Code v0.1.6), so the T3 apps can pair with it. Phase 1 of
docs/plans/t3-parity/plan.md: pairing, the environment and config, the shell and
thread streams, and thread and project lifecycle commands. Every other RPC
answers a declared failure {_tag: "UnsupportedOperationError", method}.

This is the separate binary t3-serve: `wf t3 ...` runs `t3-serve t3 ...` in its place,
found beside wf, else on PATH, so a change here rebuilds t3-serve and not wf.

State lives in --state-dir (SQLite, mode 0600): the event log with one global
sequence, command receipts, and the pairing codes, bearer tokens and tickets,
each stored only as a SHA-256 hash. A secret is never logged; the only place one
is printed is the code `wf t3 pair` mints. Two `serve` on one state directory
are not supported; `pair` may run while `serve` does.

Every verb prints one JSON object on stdout and never prompts. A failure prints
{ok: false, error, next} and exits 1; a malformed line exits 2. Server events go
to stderr as logfmt lines with static names (oauth.exchange.ok|refused,
ws.upgrade.refused, ws.opened, rpc.denied, rpc.unsupported, rpc.command.refused).

Options:
  -h, --help                  display help for command

Commands:
  serve [options]             Run the server (default 127.0.0.1:3774)
  pair [options]              Mint a one-time pairing code and its URL
  help [command]              display help for command
```

</details>

### `wf t3 serve`

Run the T3-compatible server until it is stopped. HTTP and the WebSocket share

Usage: `wf t3 serve [options]`

<details><summary>Full help</summary>

```text
Usage: wf t3 serve [options]

Run the T3-compatible server until it is stopped. HTTP and the WebSocket share
one port (default 3774) on --bind (default 127.0.0.1; the deployment binds the
tailnet address, never a LAN or public one).

HTTP: GET /.well-known/t3/environment (descriptor, orchestrationProtocolVersion 2),
GET /api/auth/session, POST /oauth/token (RFC 8693 token exchange, form-encoded:
grant_type=urn:ietf:params:oauth:grant-type:token-exchange, subject_token=<code>;
answers a 30-day bearer token with permissions, legacy scopes and scope),
POST /api/auth/websocket-ticket[/rpc|media] (Bearer; a 5-minute single-use ticket),
GET /ws?orchestrationProtocol=2&wsTicket=<ticket>. A missing or other protocol
answers 426 before any authentication; a bad, spent or expired ticket answers 401;
a bad pairing code answers 401 {code: "auth_invalid"} whatever the reason (the
reason is in the log).
GET /api/orchestration/threads/<threadId>/transcript ({threadId, title, updatedAt,
items}) and GET /api/orchestration/threads/<threadId>/history?cursor=<cursor>
[&view=conversation] ({snapshotSequence, items, nextCursor, hasMoreHistory}; at most
75 rows older than the cursor, then follow nextCursor until it is null). Items are
projected rows {position, visibility, sourceThreadId, sourceItemId, item}. Both need
the header x-t3-orchestration-protocol: 2 (426 without it) and a bearer with
orchestration:read; refusals are {_tag, code, reason, traceId}, the trace id also in
the log: 401 EnvironmentAuthInvalidError (missing_credential|invalid_credential),
403 EnvironmentScopeRequiredError, 404 EnvironmentResourceNotFoundError
(thread_not_found), 400 EnvironmentRequestInvalidError (invalid_history_cursor,
thread_transcript_too_large over 50 MiB). view=work, deferWork, compactIds and
throughEntryId are ignored.

RPCs (Effect RPC JSON framing, ack-paced streams): subscribeServerConfig,
server.getConfig, server.probe, subscribeServerLifecycle,
orchestration.subscribeShell, orchestration.subscribeThread,
orchestration.getThreadProjection, orchestration.dispatchCommand (thread.create,
thread.archive, thread.unarchive, thread.delete, thread.metadata.update; idempotent
by commandId), orchestration.launchThread and projects.mutate
(project.create|update|archive|delete). Each RPC needs the scopes of its group; a
session without them gets EnvironmentAuthorizationError. A tag in no group is the
defect "Unknown request tag: <tag>"; a known tag this server does not build fails
with UnsupportedOperationError {method}.

orchestration.launchThread creates the thread (or, with reuseExistingThread, takes
over an empty one) and dispatches initialMessage under <commandId>:initial-message;
it answers {threadId, projection, resumed, sequence}. A retry with the same
commandId writes nothing and answers resumed: true on the same thread, also when
the first launch named no threadId. workspaceStrategy root and existing_worktree
run the turn in the project root or that worktree. Not built, refused with
OrchestrationV2ThreadLaunchError {commandId, projectId, message}: worktree (a new
worktree from baseRef) and initialMessage.attachments. generateTitle and the
message's context are ignored; the title is used as given.

projects.mutate project.delete refuses a project that still has threads, active
or archived, with ProjectMutationError "Project <id> is not empty."; with force:
true it deletes them first. Every other refusal of projects.mutate is
ProjectMutationError "Failed to mutate project." (the reason is in the log).
A subscription with afterSequence replays when the client is at most 1000 events
behind, else sends a fresh snapshot.

The provider list is one provider, rca, with models from --catalog (a JSON array
of {slug, name}), else the T3_MODELS variable (slug=Name,slug=Name), else one
default.

With --rca-url (http://host:port) turns and project files go through the rca
service's agent API (/agent/v1/socket), opened with the device bearer in
T3_RCA_BEARER (wf offload bearers mint --label t3); the service secret is not
used. A message starts a turn (thread.create, turn.start, thread.subscribe on
agent.threads) and the thread's events become its messages, tool calls and run;
history and transcript reads first catch the thread up from thread.history, and
a run a restart left running is followed again at startup (one that never
reached rca fails). The turn runs in --rca-workspace, whose executor (wf exec
serve) is where the files are: projects.listEntries and projects.readFile are
file.list and file.read there, so a project's root is a path under that
executor's root, and a worktree is recorded but does not move the turn.
projects.writeFile and filesystem.browse answer an error (plan milestone M6)
until rca serves file.write and file.browse (the files.edit group); rca is not
asked. Thread metadata, pins, modes and projects stay in this server's own
store. serve checks service.status first and exits 1 when rca speaks another
protocol or lacks a group serve uses (agent.threads, files, service).
Without --rca-url nothing executes.

Output: one JSON line when it listens:
  {ok, event: "listening", address, port, environmentId, stateDir,
   orchestrationProtocolVersion}
then it runs. ok is false, with exit 1, when it cannot listen or open the store.

Options:
  --bind <address>     interface to listen on (T3_BIND; default 127.0.0.1)
  --port <n>           port (T3_PORT; default 3774; 0 picks a free one)
  --state-dir <dir>    store directory (T3_STATE_DIR; default ~/.local/share/t3-compat)
  --label <text>       environment label shown by the app (T3_LABEL; default rca)
  --catalog <file>     model catalog JSON (T3_CATALOG_FILE)
  --rca-url <url>      the rca service, http://host:port (T3_RCA_URL)
  --rca-workspace <id> the rca workspace turns and files act in (T3_RCA_WORKSPACE)
  -h, --help           display help for command
```

</details>

### `wf t3 pair`

Mint a one-time pairing code: 12 characters of 23456789ABCDEFGHJKLMNPQRSTUVWXYZ,

Usage: `wf t3 pair [options]`

<details><summary>Full help</summary>

```text
Usage: wf t3 pair [options]

Mint a one-time pairing code: 12 characters of 23456789ABCDEFGHJKLMNPQRSTUVWXYZ,
valid 5 minutes (--ttl) and redeemable once, and print the pairing URL
<base>/pair#token=<code> the T3 app opens or scans. It writes only the code's hash
to the state directory, so it works while `serve` runs. The code in the output
is the only copy: it is not logged and cannot be read back from the store.

The code is exchanged for a 30-day bearer token whose scopes are the ones given
here. Without --scope the narrowest set that lets the phone work:
orchestration:read, orchestration:operate, terminal:read, terminal:operate,
filesystem:read, filesystem:write. The other scopes (settings:write,
providers:manage, environment:maintain, preview:operate, diagnostics:read,
source-control:write, review:write, access:read, access:write, relay:read,
relay:write) only by flag.

Output: one JSON object on stdout:
  {ok, url, code, baseUrl, expiresAt, ttlSeconds, scopes, label, environmentId}
expiresAt is an ISO instant. ok is false, with exit 1, when a scope is not one of
the 17 or the store cannot be written (error and next say which).

Options:
  --base-url <url>     where the server is reached (T3_BASE_URL; default http://127.0.0.1:3774)
  --ttl <seconds>      how long the code is valid (default 300)
  --scope <list>       comma-separated scopes, repeatable; replaces the default set
  --label <text>       name of the paired client in the store (default "wf t3 pair")
  --state-dir <dir>    store directory (T3_STATE_DIR; default ~/.local/share/t3-compat)
  -h, --help           display help for command
```

</details>

### `wf test`

The e2e tests: scripted Lua tests in e2e/<name>.lua, each run through its harness (`wf android test` / `wf browser test`). A runbook is something else: plain text an agent drives. There is no queue: CI's plan job writes the plan once and each stage job (flow-tests, typecheck-lint-unit-tests, integration-tests, cli-e2e-tests, deployment-verification-tests, browser-e2e-tests, android-e2e-tests) runs its part with `wf test run --stage`.

Usage: `wf test [options] <plan|run|export|publish|results|goldens> [args]`

Examples:

```sh
wf test plan --changed --json
wf test prepare --web
wf test run --all
wf test run editor-typing --platform browser
wf test run editor-typing -- --update-goldens
wf test publish .agents/tmp/runs/<run-dir>
```

<details><summary>Full help</summary>

```text
Usage: wf test [options] <plan|run|export|drift|publish|results|goldens> [args]

The e2e tests: scripted Lua tests in e2e/<name>.lua, each run through its harness (`wf android test` / `wf browser test`).
A runbook is something else: plain text an agent drives. There is no queue: CI's plan job writes the plan once and
each stage job (flow-tests, typecheck-lint-unit-tests, integration-tests, cli-e2e-tests, deployment-verification-tests, browser-e2e-tests, android-e2e-tests) runs its part
with `wf test run --stage`.

Actions:
  plan                   every test a change needs, by CI stage; runs nothing. Scope: --all, --changed [--base <ref>] (default
                         HEAD~1; `last-green`: the newest commit on dev whose CI passed, via gh), or e2e test names; [--sha S]. Package tests (flow test:flow, integration
                         test:integration, typecheck, lint, unit test:unit, deployment-verification test:verify:deployed) follow pnpm's
                         dependency graph from the changed files; flow and integration run in the flow-tests stage. The
                         workspace root gets a typecheck when a file outside every package changed. Every focused e2e test is
                         planned, whatever changed (`platform: android, browser` makes it a test on each harness);
                         --platform android|browser keeps one harness's. --force all|<a,b>: also plans every test (all), or
                         the tests of the named packages, crates and e2e tests, whatever changed (why: forced). --json
                         prints {sha, base, stages: {<stage>: [{kind: flow|integration|typecheck|lint|unit|deployment-verification,
                         package, dir, script, command, why[]} | {kind: flow, crate, dir, manifest, command, why[]} | {kind: e2e,
                         platform, name, script, against, units[], why[]}]}, tests[]}; `why` says what picked it ("focused",
                         "changed", "depends on lib (pnpm)"). --github-output also appends
                         plan=<json> and <stage_with_underscores>=<count> to $GITHUB_OUTPUT
  run --stage <stage> [--plan <file>] [--parallel <n>] [--shard <i>/<n>] [--probe] [--fresh]
                         run one stage of a plan (default: plan HEAD~1..HEAD) through the workspace's Nx cache: a task whose
                         inputs hash the same as an earlier pass is replayed, the rest run. A package stage runs each kind's
                         target over its packages (`nx run-many -t <script> -p a,b --parallel <n>`, default 4; the flow call also runs
                         the crates' `cargo-flow`; the integration-tests stage runs test:integration and a crate's `cargo-integration`, the cli-e2e-tests stage the crates' `cargo-e2e` (a crate reached only through a dependency runs `cargo-e2e-smoke`: the same suite without its parity tests, or nothing when its suite is the live-model parity one, `tests/e2e/parity.rs`), one project at a time),
                         every kind even after a red one, and prints {ok, stage, kinds:[{kind, target, projects, verdict,
                         durationS}], expectedRevisions}; the deployment-verification-tests stage also hands its tests WF_EXPECTED_REVISIONS
                         ({service: commit}) and lists them as expectedRevisions [{app, commit, why}]: the pushed commit, or the
                         commit the service's running image was built from while the push left its package graph alone (the rule
                         of `wf ship --changed`), and the pushed one when the service cannot be asked. An e2e stage hands its
                         focused tests to `nx run-many -t e2e-<platform>`: a test runs
                         when its script, goldens, features (header `features:`, their libraries), the app shell, backends
                         (header `backends:`), lockfile or patches changed, and otherwise replays its last pass with that
                         pass's runs; it prints a line per test {test, cached, ok, units, fromSha} and then {ok, stage, ran,
                         cached, units, warms}. --probe only prints {ok, stage, ran, cached, why} (nothing runs; why holds, per missing test, the nearest earlier run and the inputs that differ, `expired` or `never-stored`: schema in `wf cache nx --help`), so the job boots
                         the device only when a test runs; test flags after `--` always run fresh, and so does --fresh (every
                         task of the stage again whatever Nx cached: the nightly CI audit's second pass, .github/workflows/
                         ci-audit.yml). --shard <i>/<n> runs the i-th of n parts of a package stage (a package or crate with all its kinds
                         goes to one part; the parts are dealt out by sorted name, so the n machines of a stage agree and together run each once). Nx also reads and writes the shared build cache through a loopback shim
                         this command starts (`wf cache --help`); --fresh never starts it. No nx in the workspace is
                         an error (pnpm install). Exit 0 only when everything passed
  run [test...]          run the named tests, --all, or those a change reaches (--changed [--base <ref>]), one after another
                         in this process; [--platform android|browser] [--sha S] [--out DIR] [--serial S] [-- <test flags>]. A test whose header has
                         `units: k=v; k=v k2=v2` runs once per unit, its pairs handed to the script as --env (keeps each run
                         short). An Android run takes the device lease and loads the EAS Update CI published on branch dev (`against: update`;
                         latest-dev.json in the verification bucket; base exp://u.expo.dev/<projectId>/group/<group>/--, which a cold
                         start opens bare). A test whose `against:` is not its harness's target is refused before anything runs,
                         naming the header line that fixes it. An Android run is an error, never a fall back to a dev server, when
                         WF_EAS_UPDATE_RESULT
                         (how this push's eas-update job ended) says failure or cancelled, when the pointer does not name this
                         push's commit after a success, or, when this push published none (skipped, or empty on a dispatch), when the
                         app's package graph changed since the pointer's commit (the rule `wf ship --changed` decides to publish by), or
                         when the update has no Android build, does not open on /, or lacks the baked lab flag, backend and agent
                         hosts and backend token, or when EAS does not serve its group to an Android device (the manifest is asked
                         when the pointer saw no 200), or when a tailnet name it baked in is no longer what `wf vm net resolve`
                         answers; each error names the next step (re-run the CI run so its eas-update job publishes: a `wf ship eas`
                         by hand lacks what CI bakes in). The app is started cold once per update group,
                         again when the group changes and after a unit that did not pass. A browser run
                         (`against: export`) gets the web export of the tree, served on loopback by this
                         command at /mla-tamagui (`wf test prepare --web` builds it; one that is missing or older than the tree is an error that
                         names it). Each run is under the test's `timeout:` (SIGTERM, a 60 s grace for the
                         report and the recording, then SIGKILL). Each run is
                         `wf <platform> test <script>`; the flags after `--` go to every one as given (--update-goldens,
                         --logpoint file:line[:condition], which is browser only: an Android run refuses it);
                         WF_TEST_PUBLISH=1 publishes each run as a bundle. Prints one line per run and then the summary;
                         exit 0 only when every run passed.
  prepare --web [--force]
                         build the web export the browser e2e tests run against, from the tree as it is: `pnpm --filter
                         mla-tamagui run export:web` in frontend/mla-tamagui/dist with source maps (the debugger's logpoints and stacks
                         resolve to original lines) and the EXPO_PUBLIC_* values of the environment baked in (CI sets the backend hosts and
                         token), stamped with a hash of the app inputs it was built from: the app, its libraries and
                         every workspace package the lockfile links it to, tests and docs aside (wf-export.json). An export in place with
                         the same inputs, baked values and flags is kept (an EAS Update exports into the same dist and replaces it);
                         --force builds anyway. Prints {ok, reused, dir, sha, inputs, builtAt, buildS, baked, flags, memory}: baked shows
                         plain URLs and flags as they are and anything else as set:<8 hex of its hash>, memory is the build's largest
                         process and, with WF_VM_SAMPLE_SECS, the VM's low MemAvailable; or {ok: false, error, next}. `wf test run` refuses
                         an export whose inputs differ from the tree's; a run's result names the baked keys and a digest, never the values.
  drift [--save <file> | --against <file> | --pins] [--targets t,t] [--projects p,p]
                         the Nx task hashes of this checkout, which decide whether a sandbox pass replays on CI; runs nothing.
                         Bare: {ok, tasks, runtime, hashes}. --save writes this side's table ({runtime, tasks: {<project>:<target>:
                         {hash, inputs}}}); --against compares it with a table the other side saved and prints {ok, compared, same,
                         drifted:[{task, inputs[]}], causes: {<input>: {kind: runtime|platform|files|env|config|dependencies, mine, theirs,
                         tasks}}, onlyHere[], onlyThere[], next}, exit 1 on any difference. Checkout path, HOME, PATH and the browsers'
                         directory never enter a hash; the platform only the tasks of a crate in the platform tier (below), the Node, pnpm and
                         Rust versions only the tasks that run them, the Chromium build only a browser test. A platform tier crate
                         (`[package.metadata.flow] platform = true`, or CPU-dependent code found by the scan; `platform = false` overrides it) hashes
                         its own cargo tasks on the platform alone, a dependent never inherits it. --pins checks node,
                         pnpm and rustc against what mise.toml pins, and every `node-version:` of .github/workflows and
                         .github/actions and the root packageManager against the same pins: {ok, pins:[{tool, pinned, actual, ok}],
                         declared:{checked, off:[{file, line, tool, version, pinned}]}, next}; the weekly audit runs it
  publish <dir>          upload one run as a run bundle to the verification-artifacts bucket under
                         verification/test-runs/<runId>/ and post its test_runs row with the bundle's report as report_path:
                         <dir> is a harness run's test output dir (report.json), a flow report dir (.flow-report) or an e2e run dir
                         (result.json). bundle.json (schema 1) lists every file's public URL, report {html,json},
                         screenshots[], recordings[], debug, timing, traces[] (the method-hit traces, trace.json, of an
                         integration chain: `wf parity coverage --traces` reads them), metrics, and the run's screen, warning
                         count and timingSummary; every recording and the report page are read back from the bucket (readBack). `cache`
                         is what the build-cache shim did in the runs that stopped in the last 15 minutes and no bundle carried yet:
                         {runs, hits, misses, puts, skipped, requests, bytes, degraded, latencyMs: {get, put, exists: {n, sumMs, avgMs, maxMs}}}, null when none did.
                         verification/test-runs/index.json lists the newest 500 runs (read through the public URL and merged,
                         never replaced). --no-record skips the row; --ran-on <host> names the runner (default WF_SITE or the
                         hostname). Needs VERIFICATION_ARTIFACTS_PAR_URL (env or vault). Prints {runId, base, bundle, report, cache,
                         files, bytes, index, recorded}.
  results                read what publish wrote; runs nothing. --query "<terms>" filters the test_runs table and the bundle
                         index, joined on runId (index runs first, completed from their row; table runs the index no longer
                         holds follow). Terms, space-separated: test=<name> unit=<k=v pairs> platform= sha=<prefix> verdict=
                         screen= runId=, any other key=value matches that --env (chunk=3), comparisons on numbers
                         (durationS>60, warnings>0, wallS<=60, timing.setup>10, timing.get_element.p95>1 in seconds, where find reads the
                         same row; = != > >= < <=),
                         a comma lists alternatives (test=editor-typing,voice-edit), newest (one run per test, unit and
                         platform), units (keeps only the runs of the current units of the focused tests the test and platform terms
                         reach, and adds {test, unit, platform, missing: true} for each such unit no selected run covers), since=<30m|6h|2d|1w|2026-10-16>, limit=<n>.
                         Equality terms on verdict, platform, sha, test and the since window go to the table as its filter.
                         --json prints {query, sources {index: ok|absent|failed: .., table: ok|disabled: ..|failed: ..},
                         runs:[{runId, test, unit, platform, sha, verdict, durationS, publishedAt, screen, warnings,
                         recordings, screenshots, readBack, wallS, phases, byVerb, timing, report, bundle, args, inIndex,
                         inTable, ts?, ranOn?}]}, newest first; without it one line per run. Exit 1 only when the index read
                         failed and the table gave nothing.
  goldens <runId|bundle URL> [--goldens-dir DIR]
                         make a published run's named screenshots (its shots/<name>.png and sidecar .json) the goldens of its
                         harness, in DIR (default docs/runbooks/goldens/<harness>/): --update-goldens for a run that happened on
                         CI, to review and commit. Prints {ok, run, platform, dir, goldens[], written[]}; exit 1 with error and
                         next when the bundle cannot be read or holds no named screenshot.

Test-run rows: publish is the only writer. Each published run posts one row to TEST_RUNS_ORDS_BASE_URL as health_rpt
(HEALTH_REPORTS_ORDS_PASSWORD, user HEALTH_REPORTS_ORDS_USER), read from the environment and wf's .env files only.
Posting never changes an exit code: an unset URL or a failed post is one `source=test-metrics msg="test run row not
posted" reason=disabled|failed` log line. `wf test device`, `wf test metrics` and `wf test record` are gone and say so.

An e2e test is a Lua file whose first lines are a header:
  --[[---
  platform: android            (or browser)
  against: update              (the EAS Update CI published: android) | export (a web export: browser) |
                               android=update, browser=export (none: update for android, export for browser)
  timeout: 180                 (seconds, default 180)
  units: problem=two-sum; problem=valid-palindrome
  focus: true
  ---]]
A Lua file without one is a helper. Focus alone decides what CI runs: every focused test runs on every push (--changed),
and only focused tests run at all (--all, or by name); focus the tests you work on and unfocus the rest.

JSON: plan {"sha","tests":[{"name","script","platform","against","focus","selected"}]};
run: one {"unit","verdict","durationS","bundle","note"} per run, then {"ok","sha","out","units":[{test, args, verdict,
note, next?, durationS, timing {wallS, byVerb}, report, recording[], bundle, base,
updateGroup, updateCommit (update units), export {dir, sha, inputs, builtAt, buildS, baked} (browser units), attempt, dir,
logTail}], "warms":[{the cold starts, one run each, with the same target fields}]}
(also written to <out>/summary.json).
```

</details>

## vm

### `wf vm`

The VM: run a script in agent-shell, read logs, recover, and its sub-areas

Usage: `wf vm [options] [command]`

Examples:

```sh
wf vm job list prints the controller line
wf vm job start --kind cheap-flow
wf vm job logs --follow
```

<details><summary>Full help</summary>

```text
Usage: wf vm [options] [command]

The VM: run a script in agent-shell, read logs, recover, and its sub-areas

Options:
  -h, --help             display help for command

Commands:
  exec [options] [file]  Run a local script inside agent-shell on the VM; by
                         default streams output until it finishes and exits with
                         its exit code
  logs [options]         Recent BetterStack log lines (default: last 10m of the
                         backend source), optionally for one service
  ledger                 Read one of the VM ledger files (allowlist:
                         opencode-dispatch-runs.jsonl, pool-cooldowns.json)
  recover [options]      Health-sweep every app via vm-ops-api and redeploy
                         unhealthy ones with recreate=all; if vm-ops-api stays
                         unreachable, OCI soft reboot, then hard reset, then
                         re-verify
  deps                   Dependency manifest of the shared agent workspace,
                         replayed at container start
  creds                  OCI credentials for Terraform
  terraform              Terraform for infra/oracle-cloud
  net                    Tailnet devices and the auth key
  ops                    Operate vm-ops-api itself (health, self-redeploy)
  help [command]         display help for command
```

</details>

### `wf vm creds`

OCI credentials for Terraform

Usage: `wf vm creds [options] [command]`

Examples:

```sh
wf vm creds
```

<details><summary>Full help</summary>

```text
Usage: wf vm creds [options] [command]

OCI credentials for Terraform

Options:
  -h, --help  display help for command

Commands:
  sync        Pull the OCI_* secrets from the vault and write ~/.oci/oci_api_key.pem
              and the matching terraform.tfvars block
  help [command]  display help for command
```

</details>

#### `wf vm creds sync`

Pull the 6 OCI_* secrets from the vault and write ~/.oci/oci_api_key.pem + the matching terraform.tfvars block

Usage: `wf vm creds sync [options]`

Examples:

```sh
wf vm creds sync --backend
```

<details><summary>Full help</summary>

```text
Usage: wf vm creds sync [options]

Pull the 6 OCI_* secrets from the vault and write ~/.oci/oci_api_key.pem + the
matching terraform.tfvars block

Options:
  -h, --help  display help for command
```

</details>

### `wf vm deps`

Declarative dependency manifest for the shared agent workspace, replayed at container boot.

Usage: `wf vm deps [options] [command]`

Examples:

```sh
wf vm deps
```

<details><summary>Full help</summary>

```text
Usage: wf vm deps [options] [command]

Declarative dependency manifest for the shared agent workspace, replayed at
container boot.

Options:
  -h, --help                display help for command

Commands:
  install <manager> <name>  Install a dependency inside agent-shell now AND
                            record it in the manifest so it survives a redeploy
  add <manager> <name>      Record a dependency in the manifest. Idempotent --
                            adding an existing entry is a no-op.
  remove <manager> <name>   Remove a dependency from the manifest.
  list                      List every entry currently in the manifest.
  replay [options]          Install every manifest entry not already present. A
                            failing entry is reported, not fatal.
  help [command]            display help for command
```

</details>

#### `wf vm deps add`

Record a dependency in the manifest. Idempotent -- adding an existing entry is a no-op.

Usage: `wf vm deps add [options] <manager> <name>`

Examples:

```sh
wf vm deps add {manager} {name}
```

<details><summary>Full help</summary>

```text
Usage: wf vm deps add [options] <manager> <name>

Record a dependency in the manifest. Idempotent -- adding an existing entry is a
no-op.

Options:
  -h, --help  display help for command
```

</details>

#### `wf vm deps install`

Install a dependency inside agent-shell now AND record it in the manifest so it survives a redeploy

Usage: `wf vm deps install [options] <manager> <name>`

Examples:

```sh
wf vm deps install apt ripgrep
```

<details><summary>Full help</summary>

```text
Usage: wf vm deps install [options] <manager> <name>

Install a dependency inside agent-shell now AND record it in the manifest so it
survives a redeploy

Options:
  -h, --help  display help for command
```

</details>

#### `wf vm deps list`

List every entry currently in the manifest.

Usage: `wf vm deps list [options]`

Examples:

```sh
wf vm deps list
```

<details><summary>Full help</summary>

```text
Usage: wf vm deps list [options]

List every entry currently in the manifest.

Options:
  -h, --help  display help for command
```

</details>

#### `wf vm deps remove`

Remove a dependency from the manifest.

Usage: `wf vm deps remove [options] <manager> <name>`

Examples:

```sh
wf vm deps remove npm agent-deps-smoke-marker >/dev/null 2>&1
```

<details><summary>Full help</summary>

```text
Usage: wf vm deps remove [options] <manager> <name>

Remove a dependency from the manifest.

Options:
  -h, --help  display help for command
```

</details>

#### `wf vm deps replay`

Install every manifest entry not already present. A failing entry is reported, not fatal.

Usage: `wf vm deps replay [options]`

| Option | Meaning |
|---|---|
| `--managers <managers>` | comma-separated subset of apt,npm,mise (run apt as root, npm/mise as the non-root user) (default: "apt,npm,mise") |

Examples:

```sh
wf vm deps replay --managers npm,mise \
```

<details><summary>Full help</summary>

```text
Usage: wf vm deps replay [options]

Install every manifest entry not already present. A failing entry is reported,
not fatal.

Options:
  --managers <managers>  comma-separated subset of apt,npm,mise (run apt as
                         root, npm/mise as the non-root user) (default:
                         "apt,npm,mise")
  -h, --help             display help for command
```

</details>

### `wf vm exec`

Run a local script inside agent-shell on the VM; by default streams output until it finishes and exits with its exit code

Usage: `wf vm exec [options] [file]`

| Option | Meaning |
|---|---|
| `--no-wait` | Start the job and print its jobId immediately, without polling |
| `--artifact <rel>` | Path of an artifact the script wrote (relative to its artifact root). Repeatable; fetched after the script exits and saved under <out>/<rel> (default: []) |
| `--out <dir>` | Directory to write <dir>/output.log, <dir>/exit-code, and every --artifact under <dir>/<rel> (created via mkdir -p) |
| `--status <jobId>` | One-shot poll of a job started with --no-wait |
| `--log <jobId>` | Print the job's whole output log (--status carries only its last 4 KB); a finished build's log stays until agent-shell restarts |
| `--cancel <jobId>` | Abort a running job (idempotent) |

Examples:

```sh
wf vm exec /tmp/p1-restart-loop.sh
wf vm exec --status
```

<details><summary>Full help</summary>

```text
Usage: wf vm exec [options] [file]

Run a local script inside agent-shell on the VM; by default streams output until
it finishes and exits with its exit code

Arguments:
  file              Local script file to run

Options:
  --no-wait         Start the job and print its jobId immediately, without
                    polling
  --artifact <rel>  Path of an artifact the script wrote (relative to its
                    artifact root). Repeatable; fetched after the script exits
                    and saved under <out>/<rel> (default: [])
  --out <dir>       Directory to write <dir>/output.log, <dir>/exit-code, and
                    every --artifact under <dir>/<rel> (created via mkdir -p)
  --status <jobId>  One-shot poll of a job started with --no-wait
  --log <jobId>     Print the job's whole output log (--status carries only its
                    last 4 KB); a finished build's log stays until agent-shell
                    restarts
  --cancel <jobId>  Abort a running job (idempotent)
  -h, --help        display help for command
```

</details>

### `wf vm ledger`

Read one of the VM ledger files (allowlist: opencode-dispatch-runs.jsonl, pool-cooldowns.json)

Usage: `wf vm ledger [options] [command]`

Examples:

```sh
wf vm ledger cat
```

<details><summary>Full help</summary>

```text
Usage: wf vm ledger [options] [command]

Read one of the VM ledger files (allowlist: opencode-dispatch-runs.jsonl,
pool-cooldowns.json)

Options:
  -h, --help      display help for command

Commands:
  cat <name>      Print a VM ledger file verbatim
  help [command]  display help for command
```

</details>

#### `wf vm ledger cat`

Print a VM ledger file verbatim

Usage: `wf vm ledger cat [options] <name>`

Examples:

```sh
wf vm ledger cat
```

<details><summary>Full help</summary>

```text
Usage: wf vm ledger cat [options] <name>

Print a VM ledger file verbatim

Arguments:
  name        Ledger basename (opencode-dispatch-runs.jsonl|pool-cooldowns.json)

Options:
  -h, --help  display help for command
```

</details>

### `wf vm logs`

Recent BetterStack log lines (default: last 10m of the backend source), optionally for one service

Usage: `wf vm logs [options]`

| Option | Meaning |
|---|---|
| `--service <name>` | Only rows tagged with this service name |
| `--since <duration>` | Look back this far: e.g. 30s, 5m, 1h, 2d (default 10m) |
| `--level <level>` | Minimum level: debug&#124;info&#124;warn&#124;error |
| `--module <name>` | Filter to a single module |
| `--grep <substring>` | Substring match on the message (case-insensitive) |
| `--limit <n>` | Max rows to return (default 200) |
| `--source <name>` | BetterStack source: backend (every backend service + wf itself) &#124; mobile (published EAS bundles). Default backend |

Examples:

```sh
wf vm logs --since 10m --level error --grep watchdog.recreated
wf vm logs --since 15m --grep watchdog.recreated
wf vm logs --service leetcode-backend
```

<details><summary>Full help</summary>

```text
Usage: wf vm logs [options]

Recent BetterStack log lines (default: last 10m of the backend source),
optionally for one service

Options:
  --service <name>    Only rows tagged with this service name
  --since <duration>  Look back this far: e.g. 30s, 5m, 1h, 2d (default 10m)
  --level <level>     Minimum level: debug|info|warn|error
  --module <name>     Filter to a single module
  --grep <substring>  Substring match on the message (case-insensitive)
  --limit <n>         Max rows to return (default 200)
  --source <name>     BetterStack source: backend (every backend service + wf
                      itself) | mobile (published EAS bundles). Default backend
  -h, --help          display help for command
```

</details>

### `wf vm net`

Tailnet device + auth-key management

Usage: `wf vm net [options] [command]`

Examples:

```sh
wf vm net auth-key-status $ARGS 2>&1
wf vm net --help
```

<details><summary>Full help</summary>

```text
Usage: wf vm net [options] [command]

Tailnet device + auth-key management

Options:
  -h, --help                   display help for command

Commands:
  device                       Tailnet device records: list, retire
  up [options]                 Join this machine to the tailnet (starts
                               tailscaled if stopped). Idempotent — safe to
                               re-run
  auth-key                     TS_VM_AUTH_KEY: status (how long it has left;
                               exits non-zero when rotation is due), rotate
                               (before Tailscale’s 90-day cap expires it)
  resolve <name...>            The name each tailnet host answers to now (the
                               base name or its -1 .. -5 drift), as JSON
  help [command]               display help for command
```

</details>

#### `wf vm net auth-key`

Rotate TS_VM_AUTH_KEY before Tailscale’s 90-day cap expires it

Usage: `wf vm net auth-key [options] <action>`

| Option | Meaning |
|---|---|
| `--apply` | actually mint and store the new key (default is a dry run) |
| `--expiry-days <n>` | key lifetime, max 90 (default 90) |

Examples:

```sh
wf vm net auth-key rotate --apply
wf vm net auth-key rotate
```

<details><summary>Full help</summary>

```text
Usage: wf vm net auth-key [options] <action>

Rotate TS_VM_AUTH_KEY before Tailscale’s 90-day cap expires it

Arguments:
  action             rotate

Options:
  --apply            actually mint and store the new key (default is a dry run)
  --expiry-days <n>  key lifetime, max 90 (default 90)
  -h, --help         display help for command
```

</details>

#### `wf vm net auth-key status`

Report how long TS_VM_AUTH_KEY has left; exits non-zero when rotation is due (read-only)

Usage: `wf vm net auth-key status [options]`

| Option | Meaning |
|---|---|
| `--warn-days <n>` | days-remaining threshold (default 14) |

Examples:

```sh
wf vm net auth-key status
```

<details><summary>Full help</summary>

```text
Usage: wf vm net auth-key status [options]

Report how long TS_VM_AUTH_KEY has left; exits non-zero when rotation is due
(read-only)

Options:
  --warn-days <n>  days-remaining threshold (default 14)
  -h, --help       display help for command
```

</details>

#### `wf vm net device`

Tailnet device records

Usage: `wf vm net device [options] [command]`

Examples:

```sh
wf vm net device list
wf vm net device retire <name>
```

<details><summary>Full help</summary>

```text
Usage: wf vm net device [options] [command]

Tailnet device records

Options:
  -h, --help        display help for command

Commands:
  list [options]    Show every tailnet device with its staleness verdict,
                    flagging collision-suffixed names
  retire [options]  Delete stale device records so the next node can claim the
                    bare hostname vm-gateway expects. DRY RUN unless --apply
  help [command]    display help for command
```

</details>

#### `wf vm net device list`

Show every tailnet device with its staleness verdict, flagging collision-suffixed names

Usage: `wf vm net device list [options]`

| Option | Meaning |
|---|---|
| `--stale-minutes <n>` | offline longer than this counts as stale (default 10) |

Examples:

```sh
wf vm net device list
```

<details><summary>Full help</summary>

```text
Usage: wf vm net device list [options]

Show every tailnet device with its staleness verdict, flagging
collision-suffixed names

Options:
  --stale-minutes <n>  offline longer than this counts as stale (default 10)
  -h, --help           display help for command
```

</details>

#### `wf vm net device retire`

Delete stale device records so the next node can claim the bare hostname vm-gateway expects. DRY RUN unless --apply

Usage: `wf vm net device retire [options]`

| Option | Meaning |
|---|---|
| `--apply` | actually delete (default is a dry run that changes nothing) |
| `--stale-minutes <n>` | offline longer than this counts as stale (default 10) |
| `--name <name...>` | restrict to these device short-names |
| `--force` | also delete ONLINE devices holding a collision-suffixed name (evicts a live node) |
| `--replaced-vm` | delete every tagged VM node record whatever its age (the selection the recreate uses; sandbox-agent is spared) |

Examples:

```sh
wf vm net device retire --name vm-ops-api-heavy-3 --force --apply
wf vm net device retire --dry-run
wf vm net device retire --apply
```

<details><summary>Full help</summary>

```text
Usage: wf vm net device retire [options]

Delete stale device records so the next node can claim the bare hostname
vm-gateway expects. DRY RUN unless --apply

Options:
  --apply              actually delete (default is a dry run that changes
                       nothing)
  --stale-minutes <n>  offline longer than this counts as stale (default 10)
  --name <name...>     restrict to these device short-names
  --force              also delete ONLINE devices holding a collision-suffixed
                       name (evicts a live node)
  -h, --help           display help for command
```

</details>

#### `wf vm net resolve`

The name each tailnet host answers to now: the base name, else the first of `<name>-1` .. `<name>-5` that resolves (a name drifts to a suffix while a stale record holds it). Resolves through this machine's resolver, so run it where MagicDNS answers (the VM's runner, a machine on the tailnet).

Usage: `wf vm net resolve [options] <name...>`

Examples:

```sh
hosts=$(wf vm net resolve leetcode-backend-prod remote-chat-agent-prod)
```

<details><summary>Full help</summary>

```text
Usage: wf vm net resolve [options] <name...>

The name each tailnet host answers to now: the base name, else the first of
<name>-1 .. <name>-5 that resolves (a name drifts to a suffix while a stale
record holds it). Resolves through this machine's resolver, so run it where
MagicDNS answers (the VM's runner, a machine on the tailnet).

Options:
  -h, --help  display help for command

Output: one JSON line {"<name>": "<the name that resolves>" | null, ...}.
Exit 1 when a name has none (named on stderr), 0 otherwise.

Example:
  wf vm net resolve leetcode-backend-prod remote-chat-agent-prod
```

</details>

#### `wf vm net up`

Join this machine to the tailnet (starts tailscaled if stopped). Idempotent — safe to re-run

Usage: `wf vm net up [options]`

| Option | Meaning |
|---|---|
| `--hostname <name>` | tailnet hostname to claim (default: sandbox-agent) |
| `--backend <name>` | secrets backend for TS_VM_AUTH_KEY (default: bitwarden off macOS) |

Examples:

```sh
wf vm net up failed (VM services unreachable
```

<details><summary>Full help</summary>

```text
Usage: wf vm net up [options]

Join this machine to the tailnet (starts tailscaled if stopped). Idempotent —
safe to re-run

Options:
  --hostname <name>  tailnet hostname to claim (default: sandbox-agent)
  --backend <name>   secrets backend for TS_VM_AUTH_KEY (default: bitwarden off
                     macOS)
  -h, --help         display help for command
```

</details>

### `wf vm ops`

Operate vm-ops-api itself (health, self-redeploy) -- not one of the apps it manages

Usage: `wf vm ops [options] [command]`

<details><summary>Full help</summary>

```text
Usage: wf vm ops [options] [command]

Operate vm-ops-api itself (health, self-redeploy) -- not one of the apps it
manages

Options:
  -h, --help                  display help for command

Commands:
  health                      GET /health -- confirms vm-ops-api is reachable
                              and which image/commit it is running
  stats                       GET /stats -- host memory and load plus every
                              running container's memory and CPU, biggest first
  logs [options] <app>        docker logs for one of vm-ops-api's managed
                              containers -- the only way to see why something
                              failed INSIDE a container on the VM, since
                              exec-script is scoped to agent-shell alone
  probe [options] <app...>    GET /health of each app on the tailnet; exit 1
                              naming the ones that do not answer, with their
                              container states
  pair [options] <app>        Mint an orca:// pairing link in the deployed
                              orca-serve container and print it (a credential:
                              pipe it)
  ci-port [options]           Check that the CI port answers /health with the
                              CI seed and 404s an admin-only route
  runner-check [options]      Check, inside a CI job on the self-hosted runner,
                              that the job has the builder sidecar and nothing
                              of the host (booleans only)
  redeploy-self [tag]         Have vm-ops-api build its OWN image at the dev
                              head and swap itself in, fire-and-forget (poll
                              `wf vm ops health` afterward to confirm)
  drill <action>              Second-door drill: stop or freeze the primary on
                              purpose and measure who brings it back
  help [command]              display help for command
```

</details>

#### `wf vm ops drill`

Second-door drill: take the vm-ops-api container down on purpose and measure who brings it back

Usage: `wf vm ops drill [options] <action>`

<details><summary>Full help</summary>

```text
Usage: wf vm ops drill [options] <action>

Second-door drill: take the vm-ops-api container down on purpose and measure who
brings it back. The service does it (POST /drill, signed, 202 before a detached
sibling acts: `stop` is docker stop after 5 s, `freeze` is SIGSTOP after 5 s with
its own thaw after 600 s). wf asserts /health on 8080 stops answering, polls it
until it answers again (limit 10 minutes), then reads the watchdog's log: by is
"watchdog" only when a successful watchdog.recreated line for that container is
newer than the drill. If nothing brings it back in time, wf reports recovered
false: there is no standby, so fix it or recreate the VM. A LIVE, mutating drill:
run it only with the owner's approval.

Arguments:
  action      stop | freeze

Options:
  -h, --help  display help for command

Output: one JSON line with --json (indented without) on stdout {ok, action,
container, down, downS, recovered, recoveredS, by ("watchdog" | "other" |
"unknown", null when not recovered), event {timestamp, container, reason, ok},
logsError, commit}. Exit 0 only when the service recovered; on failure ok is
false, error and next say what happened and the exit code is 1.

Environment: WF_VM_POLL_MS (default 5000), WF_VM_DOWN_TIMEOUT_MS (how long
/health may keep answering after the 202, default 60000) and
WF_VM_RECOVER_TIMEOUT_MS (default 600000).
```

</details>

#### `wf vm ops health`

GET /health -- confirms vm-ops-api is reachable and which image/commit it is running

Usage: `wf vm ops health [options]`

Examples:

```sh
wf vm ops health >/dev/null && echo
```

<details><summary>Full help</summary>

```text
Usage: wf vm ops health [options]

GET /health -- confirms vm-ops-api is reachable and which image/commit it is
running

Options:
  -h, --help  display help for command
```

</details>

#### `wf vm ops logs`

docker logs for one of vm-ops-api's managed containers -- the only way to see why something failed INSIDE a container on the VM, since exec-script is scoped to agent-shell alone

Usage: `wf vm ops logs [options] <app>`

| Option | Meaning |
|---|---|
| `--tail <n>` | lines to fetch (default: "200") |

Examples:

```sh
wf vm ops logs vm-ops-api
```

<details><summary>Full help</summary>

```text
Usage: wf vm ops logs [options] <app>

docker logs for one of vm-ops-api's managed containers -- the only way to see
why something failed INSIDE a container on the VM, since exec-script is scoped
to agent-shell alone

Arguments:
  app         vm-ops-api service name, e.g. vnc-auth-session

Options:
  --tail <n>  lines to fetch (default: "200")
  -h, --help  display help for command
```

</details>

#### `wf vm ops pair`

Mints a pairing link in the deployed orca-serve container (vm-ops-api runs `orca-serve pair` there; no SSH) and prints it. The link is a CREDENTIAL: it is the only thing on stdout, so pipe it and never paste it. Any app but orca-serve is refused before a request is made.

Usage: `wf vm ops pair [options] <app>`

Examples:

```sh
wf vm ops pair orca-serve | wf secret update ORCA_VERIFY_PAIR_URL --value-stdin --apply
```

<details><summary>Full help</summary>

```text
Usage: wf vm ops pair [options] <app>

Mints a pairing link in the deployed orca-serve container (vm-ops-api runs
`orca-serve pair` there; no SSH) and prints it. The link is a CREDENTIAL: it is
the only thing on stdout, so pipe it and never paste it. Any app but orca-serve
is refused before a request is made.

Arguments:
  app         orca-serve

Options:
  -h, --help  display help for command

Output: the raw orca://pair link and a newline on stdout, one human line on
stderr. On failure nothing on stdout, one JSON line {ok:false, error, next?} on
stderr and exit 1; a reply without a link is a failure.

Example:
  wf vm ops pair orca-serve | wf secret update ORCA_VERIFY_PAIR_URL --value-stdin --apply
```

</details>

#### `wf vm ops probe`

GET /health of each app on the tailnet, at the name its host answers to now (the one a web export bakes in, as `wf vm net resolve` finds it), within 5 s each. Exit 1 naming the apps that do not answer 2xx, with their container states from vm-ops-api. CI runs it before the e2e tests build or boot anything, so a service that is down fails the job in seconds, not after every unit that calls it timed out.

Usage: `wf vm ops probe [options] <app...>`

| Option | Meaning |
|---|---|
| `--wait <s>` | keep probing the apps that do not answer, every 5 s, for up to this many seconds (default 0: one probe each) |

Examples:

```sh
wf vm ops probe --wait 45 remote-chat-agent leetcode-backend
```

<details><summary>Full help</summary>

```text
Usage: wf vm ops probe [options] <app...>

GET /health of each app on the tailnet, at the name its host answers to now (the
one a web export bakes in, as `wf vm net resolve` finds it), within 5 s each.
Exit 1 naming the apps that do not answer 2xx, with their container states from
vm-ops-api. CI runs it before the e2e tests build or boot anything, so a service
that is down fails the job in seconds, not after every unit that calls it timed
out.

Arguments:
  app         leetcode-backend | remote-chat-agent | reporting-service

Options:
  --wait <s>  keep probing the apps that do not answer, every 5 s, for up to
              this many seconds (default 0: one probe each)
  -h, --help  display help for command

Output: one JSON line {ok, apps: [{app, ok, ms, error?, containers?}],
unhealthy?, next?} on stdout; on failure the same names on stderr and exit 1.

Example:
  wf vm ops probe --wait 45 remote-chat-agent leetcode-backend
```

</details>

#### `wf vm ops redeploy-self`

Have vm-ops-api build its OWN image at the dev head and swap itself in, fire-and-forget (poll `wf vm ops health` afterward to confirm)

Usage: `wf vm ops redeploy-self [options] [tag]`

Examples:

```sh
wf vm ops redeploy-self
```

<details><summary>Full help</summary>

```text
Usage: wf vm ops redeploy-self [options] [tag]

Have vm-ops-api build its OWN image at the dev head and swap itself in,
fire-and-forget (poll `wf vm ops health` afterward to confirm)

Arguments:
  tag         rejected -- redeploy-self deploys the dev head

Options:
  -h, --help  display help for command
```

</details>

#### `wf vm ops host`

What the VM's host was doing over a window, from reporting-service's stored samples over the tailnet. JSON: window, samples (sweeps), firstMs, newestMs, ageSeconds (how old the newest sweep is), gapSecondsOver, gaps [{fromMs, toMs, seconds}] (sweeps further apart than the threshold: a stall), signals {cpu, psi_io, psi_memory, psi_cpu, load1, memory_used, swap_used, list_ms} each {avg, peak, atMs}, memoryTotal, diskWriters [{container, writtenBytes}] (the five that wrote the most to disk)

Usage: `wf vm ops host [options]`

Examples:

```sh
wf vm ops host --window 24h
```

<details><summary>Full help</summary>

```text
Usage: wf vm ops host [options]

What the VM's host was doing over a window, from reporting-service's stored
samples over the tailnet. JSON: window, samples (sweeps), firstMs, newestMs,
ageSeconds (how old the newest sweep is), gapSecondsOver, gaps [{fromMs, toMs,
seconds}] (sweeps further apart than the threshold: a stall), signals {cpu,
psi_io, psi_memory, psi_cpu, load1, memory_used, swap_used, list_ms} each {avg,
peak, atMs}, memoryTotal, diskWriters [{container, writtenBytes}] (the five that
wrote the most to disk)

Options:
  --window <window>  1h, 24h, 7d or 30d (default: "24h")
  --gap-secs <n>     the longest silence between sweeps that is not a gap
                     (default: "180")
  -h, --help         display help for command
```

</details>

#### `wf vm ops stats`

GET /stats -- what holds the VM's memory and CPU. JSON: host {memTotalMb, memAvailableMb, swapTotalMb, load [1m, 5m, 15m]}, containers [{name, memMb (working set, as docker stats), memLimitMb (null: unlimited), cpuPct (of one CPU)}] biggest first, containersMb (their sum)

Usage: `wf vm ops stats [options]`

Examples:

```sh
wf vm ops stats
```

<details><summary>Full help</summary>

```text
Usage: wf vm ops stats [options]

GET /stats -- what holds the VM's memory and CPU. JSON: host {memTotalMb,
memAvailableMb, swapTotalMb, load [1m, 5m, 15m]}, containers [{name, memMb
(working set, as docker stats), memLimitMb (null: unlimited), cpuPct (of one
CPU)}] biggest first, containersMb (their sum)

Options:
  -h, --help  display help for command
```

</details>

#### `wf vm ops ci-port`

Checks, as a CI job, that vm-ops-api's CI port is the one this process uses and that it is the narrow one: GET /health answers 200 on the CI port with the CI seed (VM_OPS_API_CI_AUTH_TOKEN), and GET /services/agent-shell/logs?tail=1, a route only the admin port serves, answers 404 there. Both calls are read-only. Needs WF_VM_CI_PORT (the port number, 8081); without it every call goes to the admin port and the verb exits 1 saying so.

Usage: `wf vm ops ci-port [options]`

Examples:

```sh
WF_VM_CI_PORT=8081 wf vm ops ci-port
```

<details><summary>Full help</summary>

```text
Usage: wf vm ops ci-port [options]

Checks, as a CI job, that vm-ops-api's CI port is the one this process uses and
that it is the narrow one: GET /health answers 200 on the CI port with the CI
seed (VM_OPS_API_CI_AUTH_TOKEN), and GET /services/agent-shell/logs?tail=1, a
route only the admin port serves, answers 404 there. Both calls are read-only.
Needs WF_VM_CI_PORT (the port number, 8081); without it every call goes to the
admin port and the verb exits 1 saying so.

Options:
  -h, --help  display help for command

Output: one JSON line on stdout {ok, port, checks: [{route, expects, status,
ok, error?}]}; never a reply body. Exit 0 when both checks hold, else 1.

Environment: WF_VM_CI_PORT, VM_OPS_API_CI_AUTH_TOKEN (read from the vault when
unset).

Example:
  WF_VM_CI_PORT=8081 wf vm ops ci-port
```

</details>

#### `wf vm ops runner-check`

Read-only. Run inside a CI job on the self-hosted runner, it checks that the job has the builder sidecar and none of the host: (a) DOCKER_HOST is set and `docker info` through it answers with a server name; (b) the host docker socket does not exist; (c) PID 1's environment (names only, read from /proc/1/environ) holds no ACCESS_TOKEN, no VM_PROVISION_GITHUB_TOKEN and no BWS_ variable other than the job token BWS_CI_DEPLOY_ACCESS_TOKEN; (d) a TCP connect to the agent-shell dind address (AGENT_SHELL_DIND_ADDR, host:port) is refused or times out. No such address is configured today, so the probe is skipped and the output says so.

Usage: `wf vm ops runner-check [options]`

Examples:

```sh
wf vm ops runner-check --json
```

<details><summary>Full help</summary>

```text
Usage: wf vm ops runner-check [options]

Read-only. Run inside a CI job on the self-hosted runner, it checks that the job
has the builder sidecar and none of the host: (a) DOCKER_HOST is set and
`docker info` through it answers with a server name; (b) the host docker socket
does not exist; (c) PID 1's environment (names only, read from /proc/1/environ)
holds no ACCESS_TOKEN, no VM_PROVISION_GITHUB_TOKEN and no BWS_ variable other
than the job token BWS_CI_DEPLOY_ACCESS_TOKEN;
(d) a TCP connect to the agent-shell dind address (AGENT_SHELL_DIND_ADDR,
host:port) is refused or times out. No such address is configured today, so
the probe is skipped and the output says so.

Options:
  --proc-root <dir>          Read <dir>/1/environ instead of /proc/1/environ
                             (tests)
  --docker-sock-path <path>  Check this path instead of /var/run/docker.sock
                             (tests)
  --json                     Accepted; the output is always JSON
  -h, --help                 display help for command

Output: one JSON line on stdout {ok, docker_host_answers, docker_server,
host_docker_sock_absent, pid1_env_readable, pid1_env_clean,
pid1_offending_names?, agent_shell_dind_unreachable, agent_shell_dind_probe?,
must_not_reach?};
names and booleans, never a value. Exit 0 when every required check holds
(an unreadable PID 1 environment fails), else 1.

Environment: DOCKER_HOST, AGENT_SHELL_DIND_ADDR (optional), RUNNER_CHECK_MUST_NOT_REACH (optional: comma-separated host:port a job must not reach, e.g. the admin port of vm-ops-api; each answers unreachable, reachable or unresolved in `must_not_reach`, and anything but unreachable fails, because a name that does not resolve proves nothing).

Example:
  wf vm ops runner-check --json
```

</details>

#### `wf vm ops token-permissions`

Asks GitHub, with read-only GETs, what VM_PROVISION_GITHUB_TOKEN may do on nuesslerm/expo-hello-world-app-2: /user (is the token accepted, when does it expire), /repos/{repo} (Metadata and the permissions block), /actions/runners (a 200 proves Administration: read) and /contents/README.md (Contents: read). Nothing is created. Run it on agent-shell, which holds the token; it is the VM-side check that replaced the runner's verify of this key.

Usage: `wf vm ops token-permissions [options]`

Examples:

```sh
echo 'wf vm ops token-permissions' > /tmp/token-permissions.sh && wf vm exec /tmp/token-permissions.sh
```

<details><summary>Full help</summary>

```text
Usage: wf vm ops token-permissions [options]

Asks GitHub, with read-only GETs, what VM_PROVISION_GITHUB_TOKEN may do on
nuesslerm/expo-hello-world-app-2: /user (is the token accepted, when does it
expire), /repos/{repo} (Metadata and the permissions block), /actions/runners
(a 200 proves Administration: read) and /contents/README.md (Contents: read).
Nothing is created. Run it on agent-shell, which holds the token; it is the
VM-side check that replaced the runner's verify of this key.

Options:
  -h, --help  display help for command

Output: one JSON line on stdout {ok, repo, expires, repoPermissions: {admin,
push, pull}, probes: [{probe, needs, granted: "yes"|"no", status}]}; never the
token or a reply body. Exit 0 when the token is accepted and reads contents and
runners, else 1. Administration: write (runner registration) is not provable
without creating a token, so it is not probed.

Environment: WF_GITHUB_API (default https://api.github.com).

Example (from anywhere on the tailnet; the script is one line):
  echo 'wf vm ops token-permissions' > /tmp/token-permissions.sh && wf vm exec /tmp/token-permissions.sh
```

</details>

#### `wf vm ops cancel-run`

Cancels CI runs of dev, of both flavors (deploy.yml, self-hosted, and ci-hosted.yml, GitHub-hosted), with VM_PROVISION_GITHUB_TOKEN, because the sandbox's own GitHub token cannot. Where that token is not in the environment (a sandbox), the verb runs itself on agent-shell, which holds it, through `wf vm exec`: no script to write. --run <number> cancels that run (its run number as the Actions tab shows it, or its id); it must exist among the latest 100 runs of dev and not have completed. --queued cancels every run that is still waiting for a runner (queued, waiting, pending, requested) and leaves running ones alone. --all cancels every run that has not finished.

Usage: `wf vm ops cancel-run (--run <number> | --queued | --all) [options]`

Examples:

```sh
wf vm ops cancel-run --all
```

<details><summary>Full help</summary>

```text
Usage: wf vm ops cancel-run (--run <number> | --queued | --all) [options]

Cancels CI runs of dev, of both flavors (deploy.yml, self-hosted, and ci-hosted.yml,
GitHub-hosted), with VM_PROVISION_GITHUB_TOKEN, because the sandbox's own GitHub token
cannot. Where that token is not in the environment (a sandbox), the verb runs itself on
agent-shell, which holds it, through `wf vm exec`: no script to write. --run <number> cancels that run
(its run number as the Actions tab shows it, or its id); it must exist among the
latest 100 runs of dev and not have completed. --queued cancels every run that
is still waiting for a runner (queued, waiting, pending, requested) and leaves
running ones alone. --all cancels every run that has not finished.

Options:
  --run <number>  The CI run to cancel
  --queued        Cancel every run still waiting for a runner
  --all           Cancel every run that has not finished, running ones included
  -h, --help      display help for command

Output: one JSON line on stdout {ok, repo, cancelled: [{number, id, statusBefore,
http}], problem?}; never the token or a reply body. Exit 0 when every cancel was
accepted (HTTP 202; nothing waiting with --queued is ok), else 1.

Environment: WF_GITHUB_API (default https://api.github.com).

Example (from anywhere on the tailnet):
  wf vm ops cancel-run --all
```

</details>

### `wf vm recover`

Health-sweep every app via vm-ops-api and redeploy unhealthy ones with recreate=all; if vm-ops-api stays unreachable, OCI soft reboot, then hard reset, then re-verify

Usage: `wf vm recover [options]`

| Option | Meaning |
|---|---|
| `--dry-run` | Print the recovery plan only; make no calls |
| `--instance-id <ocid>` | OCID of the VM to reboot when vm-ops-api is unreachable (default: $OCI_INSTANCE_ID) |
| `--max-wait <seconds>` | How long vm-ops-api may stay unreachable before a reboot, and the re-verify budget (default: "600") |
| `--ref <ref>` | Git ref each recreate redeploy provisions from (default: "dev") |
| `--only <apps...>` | Sweep just these apps (default: leetcode-backend, remote-chat-agent, leetcode-auth-cookie-capture, agent-shell, vnc-auth-session, reporting-service); an app never brought up on the VM has no containers and would be redeployed forever |

Examples:

```sh
wf vm recover --instance-id
```

<details><summary>Full help</summary>

```text
Usage: wf vm recover [options]

Health-sweep every app via vm-ops-api and redeploy unhealthy ones with
recreate=all; if vm-ops-api stays unreachable, OCI soft reboot, then hard reset,
then re-verify

Options:
  --dry-run             Print the recovery plan only; make no calls
  --instance-id <ocid>  OCID of the VM to reboot when vm-ops-api is unreachable
                        (default: $OCI_INSTANCE_ID)
  --max-wait <seconds>  How long vm-ops-api may stay unreachable before a
                        reboot, and the re-verify budget (default: "600")
  --ref <ref>           Git ref each recreate redeploy provisions from (default:
                        "dev")
  --only <apps...>      Sweep just these apps (default: leetcode-backend,
                        remote-chat-agent, leetcode-auth-cookie-capture,
                        agent-shell, vnc-auth-session, reporting-service); an
                        app never brought up on the VM has no containers and
                        would be redeployed forever
  -h, --help            display help for command
```

</details>

### `wf vm state`

Backup and restore of the VM's sensitive state volumes as restic snapshots (incremental, encrypted, deduplicated) in the versioned bucket mobile-learning-vm-state.

Usage: `wf vm state [options] [command]`

<details><summary>Full help</summary>

```text
Usage: wf vm state [options] [command]

Backup and restore of the VM's sensitive state (the stateVolumes of
infra/oracle-cloud/vm-apps.json) as restic snapshots in the versioned Object
Storage bucket mobile-learning-vm-state: incremental, encrypted with
VM_STATE_RESTIC_PASSWORD, deduplicated, 14 snapshots kept per volume

Options:
  -h, --help                  display help for command

Commands:
  backup [options]            Snapshot every state volume (only what changed is
                              uploaded) and prune to the newest 14
  restore [options]           Fill an empty volume of a stopped app from its
                              newest verified snapshot
  list [options]              The snapshots of each volume, newest first
  verify [options]            Restore the newest snapshot of every declared
                              volume into a scratch directory, check it, and
                              tag it verified
  help [command]              display help for command
```

</details>

#### `wf vm state backup`

Snapshot every state volume of vm-apps.json (only what changed is uploaded) and prune to the newest 14.

Usage: `wf vm state backup [options]`

| Option | Meaning |
|---|---|
| `--app <name>` | only this app |
| `--local` | the agent-shell side: run here (needs restic, rclone, sqlite3, the OCI key, the volumes mounted); --plan <hex> carries the volumes |

<details><summary>Full help</summary>

```text
Usage: wf vm state backup [options]

Snapshots each state volume of vm-apps.json with restic: the volume is read in
place (its `excludes` are never read), its SQLite databases are copied with
`sqlite3 .backup` into a private scratch directory that is removed on every way
out and snapshotted as a second snapshot of the same run, and only the chunks
the repository does not hold are uploaded. Nothing is written to disk in
plaintext but that scratch copy.

Before reading a volume the verb measures it from the file sizes and refuses it
when it holds more than its `maxBytes` of vm-apps.json (nothing is read), and
restic refuses any file over that cap during the read; a volume that grew past
it while being read has its snapshots dropped and fails. The bucket is counted
with its previous object versions (a prune's deleted packs stay billed for a
day): a run that could take it past four times the sum of the maxBytes
(vmStateBytes of clis/blob-cache/data/budget.json) never starts, and the live
repository is held to twice the sum (vmStateRepositoryBytes), both checked
after the run too. Every file that starts with the SQLite magic bytes is copied
with `.backup`. Afterwards `restic forget --group-by host
--keep-last 14 --prune` keeps the newest 14 snapshots of every volume. The
first run creates the repository. It runs on agent-shell, which holds the OCI
key and mounts the volumes, through vm-ops-api's exec route; nothing is
prompted.

Answer (one JSON line on stdout, exit 0 only when "ok" is true):
  {"event":"vm-state","verb":"<verb>","ok":bool,"bucket":"mobile-learning-vm-state",
   "apps":[{"app":"orca-serve","ok":bool,"stamp":"20261107033000Z","ageSeconds":n,
            "volumes":[{"ok":bool,"volume":"orca-state","snapshot":"<restic id>","measuredBytes":n,
                        "maxBytes":n,"databases":n,"error":"..."}],
            "error":"...","next":"..."}],
   "repository":{"ok":bool,"bytes":n,"budgetBytes":n,"error":"..."},
   "error":"...","next":"..."}
A failed row carries the tool's own error and the next step; the restic
password is never printed.

Options:
  --app <name>  only this app
  --local       the agent-shell side: run here (needs restic, rclone, sqlite3, the OCI
                key, the volumes mounted); --plan <hex> carries the volumes
  -h, --help    display help for command
```

</details>

#### `wf vm state restore`

Fill an empty volume of a stopped app from its newest verified snapshot; never over data, never under a running app.

Usage: `wf vm state restore --app <app> [options]`

| Option | Meaning |
|---|---|
| `--app <name>` | the app whose volumes are restored (required) |
| `--local` | the agent-shell side (see backup) |

<details><summary>Full help</summary>

```text
Usage: wf vm state restore --app <app> [options]

Fills the app's state volumes from the newest snapshot that `verify` tagged
verified (and the sqlite snapshot of the same run), then recreates the app's
containers (redeploy with recreate=all from dev) so it reads the state;
agent-shell, which is deployed by hand, is not recreated.

It never extracts over data and never under a running app: it refuses when the
app has a running container (vm-ops-api is asked) and when any volume is not
empty, writing nothing, and prints the exact steps. There is no --force. A
fresh VM is restored by cloud-init before any app starts, not by this verb;
this one is for a single damaged volume of a stopped app.

Answer (one JSON line on stdout, exit 0 only when "ok" is true):
  {"event":"vm-state","verb":"<verb>","ok":bool,"bucket":"mobile-learning-vm-state",
   "apps":[{"app":"orca-serve","ok":bool,"stamp":"20261107033000Z","ageSeconds":n,
            "volumes":[{"ok":bool,"volume":"orca-state","snapshot":"<restic id>","measuredBytes":n,
                        "maxBytes":n,"databases":n,"error":"..."}],
            "error":"...","next":"..."}],
   "repository":{"ok":bool,"bytes":n,"budgetBytes":n,"error":"..."},
   "error":"...","next":"..."}
A failed row carries the tool's own error and the next step; the restic
password is never printed.
  restarted: the apps recreated; restartFailed: the ones that were not.

Options:
  --app <name>  the app whose volumes are restored (required)
  --local       the agent-shell side (see backup)
  -h, --help    display help for command
```

</details>

#### `wf vm state list`

The snapshots of each volume, newest first.

Usage: `wf vm state list [options]`

| Option | Meaning |
|---|---|
| `--app <name>` | only this app |
| `--local` | the agent-shell side (see backup) |

<details><summary>Full help</summary>

```text
Usage: wf vm state list [options]

The snapshots of each declared volume, newest first, with the run stamp, the age
and whether `verify` tagged it. A volume with no snapshot fails its app.

Answer (one JSON line on stdout, exit 0 only when "ok" is true):
  {"event":"vm-state","verb":"<verb>","ok":bool,"bucket":"mobile-learning-vm-state",
   "apps":[{"app":"orca-serve","ok":bool,"stamp":"20261107033000Z","ageSeconds":n,
            "volumes":[{"ok":bool,"volume":"orca-state","snapshot":"<restic id>","measuredBytes":n,
                        "maxBytes":n,"databases":n,"error":"..."}],
            "error":"...","next":"..."}],
   "repository":{"ok":bool,"bytes":n,"budgetBytes":n,"error":"..."},
   "error":"...","next":"..."}
A failed row carries the tool's own error and the next step; the restic
password is never printed.

Options:
  --app <name>  only this app
  --local       the agent-shell side (see backup)
  -h, --help    display help for command
```

</details>

#### `wf vm state verify`

Restore the newest snapshot of every declared volume into a scratch directory, check it and tag it verified.

Usage: `wf vm state verify [options]`

| Option | Meaning |
|---|---|
| `--app <name>` | only this app |
| `--local` | the agent-shell side (see backup) |

<details><summary>Full help</summary>

```text
Usage: wf vm state verify [options]

For every volume vm-apps.json declares: restores the newest snapshot (and the
sqlite snapshot of its run) into a private scratch directory that is removed on
every way out, restic checks each file it writes, runs PRAGMA integrity_check on
every SQLite database in it, and only then tags the snapshots `verified`. It
also runs `restic check --read-data-subset 5%` on the repository and compares
its size with the budget. A missing volume, a damaged snapshot, a database that
fails the check or a repository over budget is a failure; nothing is ever
written to a volume. The recreate preflight and the nightly workflow run it.

Answer (one JSON line on stdout, exit 0 only when "ok" is true):
  {"event":"vm-state","verb":"<verb>","ok":bool,"bucket":"mobile-learning-vm-state",
   "apps":[{"app":"orca-serve","ok":bool,"stamp":"20261107033000Z","ageSeconds":n,
            "volumes":[{"ok":bool,"volume":"orca-state","snapshot":"<restic id>","measuredBytes":n,
                        "maxBytes":n,"databases":n,"error":"..."}],
            "error":"...","next":"..."}],
   "repository":{"ok":bool,"bytes":n,"budgetBytes":n,"error":"..."},
   "error":"...","next":"..."}
A failed row carries the tool's own error and the next step; the restic
password is never printed.

Options:
  --app <name>  only this app
  --local       the agent-shell side (see backup)
  -h, --help    display help for command
```

</details>

### `wf vm terraform`

Terraform for infra/oracle-cloud

Usage: `wf vm terraform [options] [command]`

Examples:

```sh
wf vm terraform output infra/oracle-cloud
```

<details><summary>Full help</summary>

```text
Usage: wf vm terraform [options] [command]

Terraform for infra/oracle-cloud

Options:
  -h, --help                          display help for command

Commands:
  apply <workingDir> <compartmentId>  Guarded apply: free-tier check on every
                                      planned create/resize, post-apply
                                      convergence check, auto-rollback on
                                      failure.
  preflight [options] [workingDir]    Checks before a recreate: state
                                      backup, secrets, no CI run in flight,
                                      and the recreate's plan.
  exec <workingDir> [args...]         Run terraform with the remote-state
                                      credentials injected (plan, output, state
                                      list, init, import, ...). Unguarded: no
                                      free-tier check, no rollback.
  help [command]                      display help for command
```

</details>

#### `wf vm terraform apply`

Guarded apply: free-tier check on every planned create/resize, post-apply convergence check, auto-rollback on failure.

Usage: `wf vm terraform apply [options] <workingDir> <compartmentId>`

| Option | Meaning |
|---|---|
| `--recreate` | destroy and recreate the VM, clearing its tailnet records in between |

Examples:

```sh
wf vm terraform apply infra/oracle-cloud 
wf vm terraform apply --recreate [dir]
```

<details><summary>Full help</summary>

```text
Usage: wf vm terraform apply [options] <workingDir> <compartmentId>
       wf vm terraform apply --recreate [workingDir]

Guarded apply: free-tier check on every planned create/resize, post-apply
convergence check, auto-rollback on failure.

--recreate rebuilds the VM from clean and touches nothing else in the state: it
checks the remote state holds oci_core_instance.main, destroys it, deletes every
tailnet record its nodes left (so the new nodes claim the bare hostnames, never
`-1` names), then creates it again, retrying while OCI still counts the old boot
volume. The vault bootstrap token and the ADB admin password come from the
environment or Bitwarden; the deploy key is made once if missing. cloud-init then
provisions every app in main.tf's vm_apps. Prints {ok, step, retired, stderr, next}.
A failed tailnet cleanup is an error: it is retried 4 times with a growing pause,
then the command stops BEFORE the create with exit 1 and one JSON verdict
{"ok":false,"reason":...,"vmDestroyed":true,"created":false,"evidence":[{"attempt":n,
"error":...}],"next":["wf vm net device retire --replaced-vm --apply","wf vm terraform apply
<dir> <compartment>"]}: run those two commands to continue.
This is the only way to replace the VM: `wf vm terraform exec` refuses a raw
-replace of oci_core_instance.main.

Options:
  --recreate  destroy and recreate the VM, clearing its tailnet records in between
  -h, --help  display help for command
```

</details>

#### `wf vm terraform preflight`

Everything that must hold before `wf vm terraform apply --recreate`, as one JSON verdict: the state backup, the secrets, a fresh verified backup of the VM's state volumes, no CI run in flight, and the recreate's plan.

Usage: `wf vm terraform preflight [options] [workingDir]`

| Option | Meaning |
|---|---|
| `--expect <address>` | Also accept this resource being destroyed (repeatable) |

Examples:

```sh
wf vm terraform preflight
wf vm terraform preflight --expect oci_core_subnet.public
```

<details><summary>Full help</summary>

```text
Usage: wf vm terraform preflight [options] [workingDir]

Everything that must hold before `wf vm terraform apply --recreate`, as one JSON
verdict on stdout (exit 0 only when every check passed): the remote state is
pulled and backed up locally (WF_TFSTATE_BACKUP_DIR, default
~/.local/state/wf/tfstate-backups) and holds the VM; every secret group of
vm-apps.json plus the Terraform inputs and the state backup's restic password is present
and usable; every app with stateVolumes has a backup under 24 hours old that
verifies (wf vm state verify); the tailnet admin API answers with the credentials
the recreate deletes old node records with (a read-only device list: "tailnet"); no workflow run is
queued or in progress; the plan of the recreate (-replace on the VM) destroys
only the VM and the libs_read PAR, passes the free-tier guard, and lists the
actions per resource. `plan.afterApply` names the vault updates a replaced PAR
needs. Changes nothing but the local backup.

Options:
  --expect <address>  Also accept this resource being destroyed (repeatable)
  -h, --help          display help for command
```

</details>

#### `wf vm terraform exec`

Run terraform with the remote-state credentials injected (plan, output, state list, init, import, ...). Unguarded: no free-tier check, no rollback.

Usage: `wf vm terraform exec [options] <workingDir> [args...]`

Examples:

```sh
wf vm terraform exec ... -replace=oci_core_instance.main
```

<details><summary>Full help</summary>

```text
Usage: wf vm terraform exec [options] <workingDir> [args...]

Run terraform with the remote-state credentials injected (plan, output, state
list, init, import, ...). Unguarded: no free-tier check, no rollback.

Options:
  -h, --help  display help for command
```

</details>

