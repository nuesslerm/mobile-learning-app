# Re-record the real T3 and Orca servers on the VM

The cassettes `clis/t3-serve/cassettes/t3-recorded-{1,2,3}` and `clis/orca-serve/cassettes/orca-recorded` are what the pinned reference servers answered on the
VM, kind `recorded`. Nothing in them is written by hand: the inputs come from the phone coverage tables (`docs/plans/rca-parity/coverage/<app>-methods.json`),
the answers from the real server. Re-record when a pin (`tooling/t3-conformance/t3-pin.json`, `tooling/orca-conformance/orca-pin.json`), a
driver (`tooling/mimic-drivers/src`) or a corpus generator changes: the replay tests and `wf mimic verify` fail on a stale pin hash.

Everything runs through `wf vm exec <script>` (a local script run inside the agent-shell container on the VM: aarch64, Node 22.14, pnpm, git,
about 7 GB of RAM, 2 cores, no desktop, no cargo). A script body is at most 64 KiB; artifacts the script writes under
`/home/agent/e2e-artifacts` come back with `--artifact <rel> --out <dir>`. No credentials are involved: the servers run on throwaway homes with an
empty environment, and the pairing material never leaves the VM's temp dirs (the cassettes hold masked values).

## Trigger a capture from a commit (any set, any app)

A record or a replay of any set runs on a GitHub-hosted runner without an agent session driving it: push an EMPTY commit to `dev` whose message carries a
marker. `mimic-record.yml` starts only for such a commit (the gate of `scheduled-triggers.yml`: the commit changes no file, so the CI pipeline starts no run
either) and runs niced.

```sh
git commit --allow-empty -m "[record t3-recorded-1 orca-recorded]" && git push origin dev    # record two sets, one after the other
git commit --allow-empty -m "[record orca-recorded chunk=15 fresh]" && git push origin dev   # 15 minute chunks, ignore an earlier checkpoint
git commit --allow-empty -m "[replay t3-recorded-1]" && git push origin dev                  # replay against the candidates, diff table in the summary
```

Grammar, one marker per message on a line of its own that begins with `[` (a mention of the text mid-line in prose starts nothing), parsed by `wf mimic trigger` (`--help` is the one place it is written down): `[record <set>... [chunk=<minutes>] [fresh]]` or
`[replay <set>...]`. A `<set>` is a cassette set directory with a `mimic.toml`, by its name or by its path from the repository root when two share a name. A malformed
marker, an unknown set (the error lists the known ones), a record of a set with no `[reference]`, corpus or `[provision]`, or a replay of a set with no cassette or candidate,
fails the first job with the reason in the run summary.

What a run does per set, one at a time (`max-parallel: 1`): sets up the toolchains the set pins (node, python, go, rust), runs `wf mimic provision` (the pinned source and the install and build steps of its
`[provision]` block, reused while that block and commit are unchanged), then `wf mimic job record` (`wf mimic record <set> --resume --max-minutes <chunk>` in a loop,
default 10 minutes a chunk, 120 minutes of chunks, hard job timeout 240) or `wf mimic job replay`. A record that runs out of its budget uploads its checkpoint (`<set>/raw/checkpoint`) as an
artifact kept 3 days and exits red (a failed leg uploads nothing, and neither does a checkpoint with a token-shaped value in it); the same marker again restores the newest checkpoint of `dev` and goes on, but only when it was recorded against the same provision block (with its source commit), corpus and reference pin: `wf mimic job` drops any other and starts over, and `fresh` is the only way to drop a checkpoint on purpose. A published record deletes its checkpoint artifacts. A finished record has `raw/` deleted, is verified, is scanned for token-shaped values (a hit holds it
back) and is published twice: as the workflow artifact `mimic-set-<set>` (14 days), and to the verification-artifacts bucket as a run, which works from an agent session whose
proxy refuses the Actions API. The run summary has the counts, the stability (`1 - nondeterministic / interactions`), the wall time and the commands:

```sh
wf mimic fetch <run-id>               # or: wf mimic fetch latest --set <name>
wf mimic verify clis/t3-serve/cassettes/t3-recorded-1
git add clis/t3-serve/cassettes/t3-recorded-1 && git commit -m "mimic: re-record t3-recorded-1"
```

`wf mimic record --resume --max-minutes <n>` is what the chunked job calls: it stops cleanly at the first batch boundary after `n` minutes (a decimal is fine, and every call records at least one batch), leaves its state under `<set>/raw/checkpoint` (bound to the corpus, rules, seed and batching: a changed one refuses to resume) and prints `complete:false`; the call that finishes prints `complete:true`. A killed call loses at most its current batch. It is the same checkpoint the crawl sets use (`--chunk`, `--write-chunk`), so every set kind (`t3-recorded-*`, `orca-recorded`, the scenarios and the crawls) goes through one path.
A `wf-mimic` without it fails a `[record ...]` at once with a message naming both flags; `[replay ...]` does not need it. Why a dispatch is not used:
`docs/gaps/record-trigger-without-dispatch.md`.

### Add a new clone: data, not workflow YAML

A new reference (another CLI or server) is a cassette set whose `mimic.toml` says how a runner installs it, next to a `[reference]` driver that speaks `mimic-driver/1`. Either an inline
`[provision]` table or `provision_file = "<path>"` (sets of one app share a file: `tooling/mimic-drivers/provision/t3.toml` and `orca.toml` are the two in use):

```toml
workdir = "orca"                    # under the work root, default the set's name

[toolchain]                         # set up first by the workflow, checked again by `wf mimic provision`: node, python, go (24, 22.14, 22.14.0) and rust (those or stable)
node = "24"
# drivers = false                   # default true with node: the repository's own mimic drivers are installed before a record's steps

[source]                            # optional: the reference's own repository, at a pinned commit
repo = "https://github.com/stablyai/orca"    # https, no credentials
pin = "../../orca-conformance/orca-pin.json" # commit_pointer = "/commit" by default; or commit = "<40 hex>"; a floating ref is refused

[env]                               # what the [reference] command reads; never a credential
MIMIC_ORCA_ROOT = "{source}"

[[step]]                            # run in order, no shell; {repo} {work} {source} {set} are filled in
name = "install"
run = ["pnpm", "install", "--frozen-lockfile"]
cwd = "{source}"                    # default {source}, else {work}
when = "record"                     # record (default), replay (builds the candidate), always
timeout_minutes = 45                # 1 to 180, default 30
```

Check it offline with `wf mimic provision <set> --check`; the flow test `every_set_of_the_repository_that_declares_provisioning_validates` does so for every set in the repository.
Steps run with the caller's environment minus every credential-looking variable and write to stderr. A set's `[env]` is refused when it names a variable that changes how programs are found or loaded (`PATH`, `HOME`, `LD_*`, `NODE_OPTIONS`, `*_PROXY`, ...), and it reaches the record through `wf mimic job` only, never `$GITHUB_ENV`; provision and capture run before the vault is set up. A clone of a Rust, Go or Python program pins its toolchain in `[toolchain]` and needs no YAML. `provision_file` is relative to the `mimic.toml`, or to the repository root when it starts with `//` (`//tooling/mimic-drivers/provision/t3.toml`).

`wf mimic job record` runs the record with `--recorded-on ci` and the stability check on. A set whose reference is not deterministic says so, and adds flags, in its `mimic.toml`:

```toml
[record]
allow_nondeterministic = true       # default false
args = ["--keep-raw"]               # extra `wf mimic record` flags (never --resume, --max-minutes, --recorded-on, --source)
```

With `allow_nondeterministic` (or without it, when the record fails) the job output carries `differences`: for each interaction whose two stability passes differed after masking (the first 40), the first 8 JSON pointers into the interaction where they differed, as `{"path": "/steps/0/payload/id", "first": "string(len 36)", "second": "string(len 36)"}`. Only the kind and length of each side is shown, never the value (a capture may hold a secret). Which interactions vary changes from run to run (only the sampled ones are run twice, and ids, timestamps and counters differ each time), so look for the same path or key recurring across interactions and mask that, not the interaction. Add to `rules.json`, using only the kinds `wf mimic --help` lists: a `pointer` rule (`{"kind": "pointer", "pointer": "/steps/0/payload/id", "interaction": "thread.pin"}`; the pointer is the reported path, `interaction` is optional and scopes it to one id), or, when the same key recurs, `volatile-number` / `volatile-string` with `{"keys": {"<key>": "<placeholder>"}}`, `timestamp` with `keys`, `sequence` with `keys`, or `token` with `pattern` and `placeholder`. Then push the marker again; the list of differences shrinks to what is still unmasked.
 Not yet declared:
the `t3-scenarios` and `orca-scenarios`-style sets that need a scripted CLI built first (`T3_CLI_DIR`): add a step that builds it, then the block.

### Land new rules together with the re-record

A rules change makes the committed sets stale: `wf mimic verify` (and the pre-commit hook, which runs it for any staged file of a set) fails on `rules_hash`, and the record job only sees the rules that are on `dev`, because a marker commit changes no file. So the rules go first, with the set they were written for, and the record follows:

1. Edit each `rules.json` (a `pointer` rule per reported path, scoped with `interaction`; the job lists at most 8 paths per interaction, so a list of exactly 8 may be cut: check the sibling keys of the same object in the committed set). A pointer names a whole subtree too (`/steps/10/payload/exit/cause/0/error/cause`), which is how error text that depends on the state the first pass left is masked; the better fix is a scenario whose two passes see the same state, because a masked error text is no longer evidence of the error. An answer whose frame count differs (`/steps` of another length) has no mask: it stays tagged `nondeterministic` and listed in `stability-allowlist.json`.
2. `wf mimic remask <set>` for each edited set: the committed interactions masked by the new rules and the rules hash re-sealed, offline (`changed` names the interactions the new rules reached). Commit `rules.json`, `cassette.json` and `interactions.jsonl` together; never edit the last two or a hash by hand.
3. Push the marker once that commit is on `dev`: `[record <sets>]`, or `[track t3 orca]` while a pin is still behind (a track records only a drifted app, with the pin bumped in the runner).
4. The leg of a set that allows nondeterminism is green when its only failing check is the stability accounting (`verify.accounting_deferred` in the job output): the allowlist on `dev` names the old run's tags, and the new run tags others. The set is uploaded and published.
5. `wf mimic fetch <run-id>`, then `wf mimic verify <set>` names every tagged id that is not listed and every listed id that is not tagged: rewrite `stability-allowlist.json` to exactly the tagged ids, each with its cause (`array-length` for a frame count) and a reason. This is the one step that regenerates the allowlists. For a `[track]` run also copy the pin files and coverage tables of the `track-contracts` artifact, so the pin hash of the set matches the committed pin. Commit; the hook verifies.

## 0. Inputs (local): crawl the pinned source

The corpus is derived, never listed by hand (plan R2). `wf mimic crawl` reads the pinned source of each reference and writes, beside a `crawl.toml`,
`inventory.json` (every endpoint, who uses it, where it was seen), `corpus.json` (valid, boundary and invalid inputs per endpoint) and `coverage.json`
(every endpoint: its cases or why it is skipped):

```sh
# the reference trees: references/t3code and references/orca at the pins (pnpm run submodule:init), or a checkout elsewhere
MIMIC_T3_ROOT=<references/t3code at the pin> MIMIC_ORCA_ROOT=<references/orca at the pin> \
wf mimic crawl clis/t3-serve/cassettes/t3-api clis/orca-serve/cassettes/orca-api
wf mimic crawl --all --check            # offline: the committed files still cover every row of the catalogs (also run by the pre-commit gate)
wf mimic crawl --all --check --fresh    # with the trees checked out: the same crawl again is byte-equal
```

The guard fails when the catalog of the pinned source (`docs/plans/rca-parity/coverage/<app>-methods.json`, from `wf parity coverage`) lists a row the
crawl lacks, an endpoint has no case and no skip reason, a schema-derived endpoint lacks a valid or an invalid input, or the pin moved since the crawl.
What the crawl cannot see is listed per app under `blind` in `inventory.json` with the source that would show it (a running server, the release
binary's `--help`); capture those on the VM and feed them back with `wf mimic crawl <dir>` after adding `--runtime <captured dir>` to the `command` of
`crawl.toml` (`descriptor.json` and `help*.txt` files of `t3 --help` and each subcommand).

The older `node src/cli.ts corpus <app> <set>` (the phone rows only, the R1 recording sets) is superseded by the crawl and kept only until the
recorded sets are re-recorded from `t3-api` and `orca-api`.

## 0b. Inputs (local): derive the stateful scenarios

The crawl asks every method against an empty server (T3: after the session prelude). The scenario sets ask the methods that need stored state with that
state present: `node tooling/mimic-drivers/src/cli.ts scenarios <orca|t3>` reads the committed crawl (no reference tree needed) and writes `corpus.json` and
`scenarios.json` of `clis/orca-serve/cassettes/orca-scenarios` and `clis/t3-serve/cassettes/t3-scenarios`. A method whose valid payload names an entity key
(Orca `repo`, `worktree`, `terminal`; T3 `threadId`, `projectId`, `cwd`, and `messageId`/`runId` after a turn) gets the chain that creates the entity,
then itself with the captured id, then a list readback; the stories (repo, worktree, files, git, terminal, settings and host state for Orca; thread and
project lifecycles and one scripted turn for T3) only choose methods and their order, every payload is the crawl's. `scenarios.json` lists each endpoint as
`populated` (with the scenarios that ask it) or `empty-only` with a reason (`stateless`, `unresolved-entity`, `unreadable-schema`, `streaming`,
`not-recorded`); `tooling/mimic-drivers/test/scenarios.test.ts` fails when the committed files lag the generator, when an entity-naming method has no
scenario, or when the crawl lacks a payload a scenario needs.

Each scenario is one `script` case, so one fresh server, user data and HOME. Orca `setup: [{"git":"main"}]` makes a real one-commit repository (fixed
dates, so the commit id is stable) and `{git.main}` is its path; the scratch and user-data paths are masked in the steps. T3 turns need the scripted CLI:
build exec-testkit's `scripted-cli` once and export `T3_CLI_DIR=<dir holding its claude/codex>` for the record (the driver puts it first on the server's
PATH; no model credentials). Record like any set: `wf mimic record clis/<crate>/cassettes/<app>-scenarios` (section 3), then `wf mimic stabilize`.

## 1. Once per VM: the pinned T3 release, Node 24 and the pinned Orca build (about 5 minutes)

T3 is the pinned release (`t3-pin.json`: version, sha256 of the linux-arm64 tarball); the Provisioner (`tooling/mimic-drivers/src/t3/release.ts`) downloads it
into `T3_RELEASE_CACHE` and refuses it when the pin and the release's `SHA256SUMS` do not both name the tarball's hash.

Orca needs Node 24 (the VM has 22) and is built from the pinned commit; the VM has no checkout, so fetch exactly the pin:

```sh
#!/bin/sh
# wf vm exec orca-build.sh
set -e
mkdir -p /tmp/r1-spike/node24 /tmp/r1-spike/orca/src
V=$(curl -fsSL https://nodejs.org/dist/index.json | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).find(x=>x.version.startsWith("v24.")).version))')
curl -fsSL https://nodejs.org/dist/$V/node-$V-linux-arm64.tar.gz | tar xz -C /tmp/r1-spike/node24 --strip-components=1   # no xz on the VM: the .tar.gz
export PATH=/tmp/r1-spike/node24/bin:$PATH
cd /tmp/r1-spike/orca/src && git init -q && git remote add origin https://github.com/stablyai/orca
COMMIT=dc7cd7bebbf9eb45d79edc89f3f836c75be78dea   # `commit` of tooling/orca-conformance/orca-pin.json
git fetch --depth 1 origin $COMMIT && git checkout -q --detach $COMMIT
pnpm install --frozen-lockfile
pnpm -C mobile install --frozen-lockfile
pnpm -C . run build:mobile-web
pnpm -C . run build:orcad
```

Keep `COMMIT` equal to the pin: the Orca Provisioner (`src/orca/orcad.ts`) refuses a checkout that is not at the pin.
`build:orcad` fails without the mobile web bundle, hence the two mobile steps before it. The native `node-pty` prebuild compiles on arm64 unaided.

## 2. Stage the drivers and the sets on the VM

The VM's CI-published `/opt/clis/current/wf` and `wf-mimic` do the recording, so no Rust is built there. The drivers are a small tree (21 KB gzip'd) plus a pruned
`pnpm-lock.yaml` of the four pinned dependencies (same `toolchain_lock_hash`). Build one tarball, send it in chunks under 64 KiB, unpack:

```sh
STAGE=$(mktemp -d)
mkdir -p $STAGE/tooling $STAGE/clis/t3-serve $STAGE/clis/orca-serve $STAGE/docs/plans/rca-parity
tar -C tooling --exclude node_modules -cf - mimic-drivers orca-conformance/orca-pin.json t3-conformance/t3-pin.json | tar -C $STAGE/tooling -xf -
cp -r clis/t3-serve/cassettes $STAGE/clis/t3-serve/ && cp -r clis/orca-serve/cassettes $STAGE/clis/orca-serve/
cp -r docs/plans/rca-parity/coverage $STAGE/docs/plans/rca-parity/
node .agents/tmp/vm/prune-lock.mjs $STAGE/pnpm-lock.yaml     # importers + packages of tooling/mimic-drivers only
tar czf - -C $STAGE . | base64 -w0 | split -b 60000 - $STAGE.part.
# for each part N: a script `base64 -d`-ing it onto /tmp/r1/stage.b64 (append), then `base64 -d < stage.b64 | tar xz -C /tmp/r1/stage`
```

Then on the VM: `cd /tmp/r1/stage/tooling/mimic-drivers && PATH=/tmp/r1-spike/node24/bin:$PATH npm install --no-audit --no-fund` (zod, tweetnacl, @noble/hashes, esbuild).

## 3. Record the R1 slices (detached; superseded by section 6 for anything long)

Write `/tmp/r1/run-t3.sh` and `/tmp/r1/run-orca.sh` (one per app; `<app>` is `t3` or `orca`, `<crate>` is `t3` or `orca-serve`), then start both detached with one script:

```sh
#!/bin/sh
# /tmp/r1/run-<app>.sh
export PATH=/tmp/r1-spike/node24/bin:/opt/clis/current:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin MIMIC_ORCA_ROOT=/tmp/r1-spike/orca/src ORCA_ORCAD_CHECKOUT=/tmp/r1-spike/orca/src T3_RELEASE_CACHE=/tmp/r1-spike/t3-cache
cd /tmp/r1/stage/clis/<crate>/cassettes
date +%s > /tmp/r1/<app>.start
wf mimic record <app>-recorded --recorded-on vm --allow-nondeterministic --keep-raw --source "R1: every phone row, real server, scripted inputs" > /tmp/r1/full-<app>.json 2> /tmp/r1/full-<app>.err
echo $? > /tmp/r1/<app>.exit
date +%s > /tmp/r1/<app>.end
```

```sh
#!/bin/sh
# wf vm exec launch.sh
setsid nohup sh /tmp/r1/run-t3.sh   >/tmp/r1/run-t3.out   2>&1 </dev/null &
setsid nohup sh /tmp/r1/run-orca.sh >/tmp/r1/run-orca.out 2>&1 </dev/null &
```

Poll with a script that prints `/tmp/r1/<app>.exit` and `.end` (a missing file means still running). `wf mimic record` runs every corpus case twice, each against a fresh
server, and writes the set only if the two runs agree after masking (`rules.json`); with `--allow-nondeterministic` the unstable cases are written tagged
`nondeterministic` and are never evidence. Wall time on the VM is in the audit (T3 starts a server per case).

## 4. Bring the sets back and check them

```sh
# on the VM: tar czf /home/agent/e2e-artifacts/r1/{t3,orca}-set.tgz -C <stage>/clis/<crate>/cassettes <app>-recorded
wf vm exec pack.sh --artifact r1/t3-set.tgz --artifact r1/orca-set.tgz --out /tmp/r1-art
tar xzf /tmp/r1-art/r1/t3-sets.tgz  -C clis/t3-serve/cassettes
tar xzf /tmp/r1-art/r1/orca-set.tgz -C clis/orca-serve/cassettes
rm -rf clis/*/cassettes/*-recorded*/raw    # raw/capture.jsonl holds the unmasked run: never commit it
for s in clis/t3-serve/cassettes/t3-recorded-* clis/orca-serve/cassettes/orca-recorded; do wf mimic verify $s; done   # offline: hashes, kind, pin
grep -lE 'eyJ[A-Za-z0-9_-]{10}|Bearer [A-Za-z0-9]{8}' clis/*/cassettes/*-recorded*/interactions.jsonl   # must print nothing: tokens are masked
```

`wf mimic verify <set> --live` (on the VM, same environment as step 3) re-asks the real server and fails on drift; it is never part of the gate.

## 4b. Stabilize: infer the masks from consecutive recordings (R2)

A set that records with `nondeterministic` cases is not evidence for those cases. `wf mimic stabilize` turns the difference between consecutive recordings of
the same corpus into reviewable rules instead of hand-written ones. Record the affected cases (or the whole corpus) three times under the set's current
rules, each with `--allow-nondeterministic`, niced and one server at a time (script `r2-run.sh` below), copy each written set to its own directory (drop `raw/`: it
holds the unmasked capture), bring them back (step 4), then locally:

```sh
wf mimic stabilize clis/t3-serve/cassettes/t3-recorded-1 /tmp/r2/t3-new-1 /tmp/r2/t3-new-2 /tmp/r2/t3-new-3            # dry run: the proposals and what stays unstable
wf mimic stabilize clis/t3-serve/cassettes/t3-recorded-1 /tmp/r2/t3-new-1 /tmp/r2/t3-new-2 /tmp/r2/t3-new-3 --write    # rules.json, masked interactions, tags, stability.json
```

The first recording is the set that is rewritten; the others are only compared (cases missing from one are `unobserved`, not judged). The proposals are in
`stability.json` with the case that showed each and why; a value that shares its shape with an answer that never varies becomes a `pointer` rule scoped to
that interaction, never a global mask. Every case that still differs must be listed in the set's `stability-allowlist.json` with the cause the stabilizer finds and a reason
(`ok` is false otherwise, and for a listed case that agrees now). Sources of nondeterminism are fixed in the drivers first (a fresh home per server, `TZ=UTC`,
`MIMIC_FIXED_PORT` for one fixed port when a single server runs at a time) and masked second.

## 5. Replay and settle the gaps (local)

```sh
cargo test -p t3-serve --test integration recorded_r1          # replays t3-recorded-* against t3-serve, a fresh chain per interaction (about 4 minutes)
cargo test -p orca-serve --test integration recorded_r1     # replays orca-recorded against orca-serve (25 s)
```

Each test fails on a difference that `known-gaps.json` of the set does not list, and on a listed gap that no longer differs as listed. After an adapter fix,
delete its entry; after a re-record, start from `{"schema":"mimic-known-gaps/1","gaps":[]}` in each set, run once with `WF_TEST_OUTPUT_DIR=<dir>` to get
`<suite>/recorded-diff.json` (every differing interaction with its paths) and re-derive each set's `known-gaps.json` from it (id, row, severity, class, `path kind severity` lines).
Then refresh the evidence from a COMPLETE run: the integration binary must list only tests that use `exec_testkit::case!` or the run is never complete, so
`WF_TEST_OUTPUT_DIR=<dir> cargo test -p orca-serve --test integration -- --skip mimic_x --skip recorded_r1`, then `-- mimic_x2 recorded_r1` into the same dir, then
`wf parity coverage --app <t3|orca> --traces <dir>` (for t3 the plain `cargo test -p t3-serve --test integration` run is complete) and commit the tables. A row is decided `oracle: mimic`
in `<app>-decisions.json` only when its replay is clean and the recorded answer was a success; remove the mark of a row the refresh leaves pending.

## 6. Record the whole crawl in short foreground calls (R2c)

The old record path started a real server per case and ran every case twice (about 4 s Orca, 10 s T3 per start: hours, and a detached job that dies halfway). The path now keeps one server for many cases: read-only cases run many at once, `state:write` cases run last in chunks with a server reset between them, `state:fresh` cases (every scenario) get a pristine server, and only a stability sample (`--stability <pct>`, state-free cases only; write cases are never run twice inside a recording, so a recorded answer does not depend on the sample) is run twice. The crawl sets carry `[record] write_chunk = 40, stability_pct = 8` in `mimic.toml`, so a bare `--chunk` call is bounded. A run is cut into calls of bounded length and resumed from a checkpoint file in the set dir:

```sh
# each call is one foreground `wf vm exec` of under 10 minutes, niced; repeat until the JSON says "done": true
nice -n 19 wf mimic record <app>-crawl --recorded-on vm --chunk 400 --resume --write-chunk 40 --stability 8 \
  --source "R2c: <app> crawl" > /tmp/r2c/<app>.json
```

`--resume` carries on from the checkpoint (a killed call loses at most its current batch; the checkpoint is bound to the corpus, rules and plan, so a changed one refuses to resume); the last call writes `cassette.json` and `interactions.jsonl` and removes the checkpoint. Run one real server at a time, only while `wf vm ops host --window 1h` shows an idle VM and no CI run is in progress (a CI run starves the 2-core VM), and never leave a call running unattended. Measured on the VM: one full pass about 2 minutes (Orca) and 5 to 8 minutes (T3, longer while other jobs run); the scenario sets cost a server start per scenario (about 40 minutes for both). The cross-order stability study is `wf mimic stabilize` over three recordings of a stratified subset; remaining unstable rows go to `stability-allowlist.json` with their cause. Endpoints that cannot be recorded (Orca: 25 streaming methods whose script op carries request and response only, 7 third-party URLs, 1 e2ee handshake every case already opens; T3: 2 that replace the installed server binary and 1 that commits a desktop update) stay in the corpus report as not recordable.

## 7. Replay the crawl against the adapters (R2c)

```sh
MIMIC_ORCA_ROOT=<a checkout of references/orca at the pin> cargo test -p orca-serve --test integration the_answers_the_real    # orca-recorded and orca-crawl: about 77 s wall, 68 s CPU
cargo test -p t3-serve --test integration the_answers_the_real                                                                      # t3-recorded-* and t3-crawl: about 12.5 min wall, 59 s CPU
```

The crawl's reads share one chain and its state-changing cases another, in cassette order (a T3 interaction replays the recorded session prelude once per chain and then runs on a socket of its own). Each test fails on a difference its set's `known-gaps.json` (`mimic-known-gaps/2`) does not list and on a listed gap that no longer differs as listed. To re-derive a crawl set's gaps after a re-record or an adapter fix: `WF_MIMIC_WRITE_GAPS=<set> cargo test ...` writes the file (grouped by identical difference, classes derived from the path and kind of the worst difference), review the diff and commit it; the hand-curated recorded slices are never overwritten. The honest diff table by severity is `docs/audits/2026-11-07-mimic-r2c-replay.md`; each interaction id in a gap group is an adapter bug or an unserved method until the adapter is fixed. The replays write `wf-mimic-orca-crawl/trace.json` (one test per interaction) into `WF_TEST_OUTPUT_DIR`; the evidence tables are refreshed from a complete run only (section 5).
