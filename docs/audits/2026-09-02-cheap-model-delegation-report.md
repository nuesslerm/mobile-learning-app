# Cheap-model delegation: what it costs, what it saves, what to do

Date: 2026-09-02. Scope: PR #236 batch 5. Full record with every command and
transcript reference: `docs/known-gaps.md` #51. Numbers are one run per cell
unless stated; treat ratios as direction, not measurement.

## Headline

| Finding                                                                          | Number                                                      |
| -------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| Whole task to opencode (W) vs unrestricted Claude (A), 4 tasks, all gates passed | $0.014 vs $0.370, about 27x cheaper                         |
| Step-by-step delegation over Bash with skill + rules + reminder (S) vs A         | $0.846 vs $0.370, 2.3x more expensive, lost on 4 of 4 tasks |
| Same setup with the MCP `delegate` tool available (M) vs A                       | $0.396 vs $0.370, about even, delegated on 3 of 4 tasks     |
| Bare haiku session, no delegation (H) vs A                                       | $0.207 vs $0.370, about half                                |
| First-turn cache write per fresh session after the context diet                  | 30,201 to 8,836 tokens (-71%)                               |
| Fixed tokens before the first tool call after the diet                           | 55,610 to 35,614 (-36%)                                     |
| Everything opencode did today (153 runs: dispatches, judge calls, benchmarks)    | 1,017,809 tokens, $0.110                                    |

## What was measured

Three instruments, three questions.

**Delegation benchmark** (`agent-evals delegation-benchmark`): what does a
task cost on Claude's budget under six setups. Each task runs in a clean
scratch directory holding only that setup's config. Cost is `total_cost_usd`
as reported by `claude -p --output-format json`. Quality is a content check
run by the harness plus, for the index task, an external gate script that
verifies every note is linked in order with its title. Tasks: add an email
validator with a test (4 files), rename a constant across 4 files, trace a
request path and report it, generate an INDEX.md for 25 notes.

Conditions: A unrestricted; B the opencode-dispatch skill plus a hyperemphasized
CLAUDE.md; S = B plus the always-loaded rules file plus the per-turn reminder
hook, Bash surface only; M = S plus the plugin's MCP server; W the prompt sent
straight to opencode with no Claude session; H a bare `claude -p --model haiku`.

**Routing eval** (`agent-evals opencode-eval --surface=cli|mcp
[--cheap-mode]`): does a session in this repo delegate on its own, and through
which surface. Real sessions in a worktree of the repo, a stub opencode so only
the decision is measured, `total_cost_usd` per session, a probe for whether the
MCP tool had to be found via `ToolSearch`.

**Run log** (`wf opencode stats`): opencode's own reported tokens, cost, model
and wall-clock for every dispatch.

## Benchmark results

Claude-side cost in USD per run (W shows opencode's cost); seconds are the whole
run. Every cell passed every check.

| Task                   | A             | B             | S              | M             | W              | H             |
| ---------------------- | ------------- | ------------- | -------------- | ------------- | -------------- | ------------- |
| Add validator + test   | $0.123 / 25 s | $0.245 / 66 s | $0.253 / 147 s | $0.126 / 62 s | $0.0046 / 30 s | $0.064 / 36 s |
| Rename across 4 files  | $0.076 / 12 s | $0.152 / 28 s | $0.193 / 79 s  | $0.104 / 21 s | $0.0034 / 14 s | $0.045 / 25 s |
| Trace a request path   | $0.078 / 14 s | $0.191 / 73 s | $0.129 / 29 s  | $0.084 / 44 s | $0.0029 / 12 s | $0.047 / 24 s |
| Index 25 notes (gated) | $0.093 / 16 s | $0.240 / 63 s | $0.271 / 126 s | $0.082 / 47 s | $0.0031 / 13 s | $0.051 / 32 s |
| Sum                    | $0.370        | $0.828        | $0.846         | $0.396        | $0.014         | $0.207        |
| Delegations observed   | 0             | 4             | 5              | 3             | 4              | 0             |

Reading:

1. Per-step delegation from a Sonnet session over Bash (B, S) never saved money
   on tasks this size. The orchestrator pays to write the prompt, read the
   summary and verify, on top of the cheap model's turn. The reminder made it
   worse and slower.
2. Handing the whole task to opencode (W) cost a third of a cent per task and
   passed every check including the external gate, in comparable wall-clock.
3. The MCP tool (M) is the one delegation path inside a Claude session that does
   not cost extra: a structured call with a compact result replaces the
   prompt-file plus Bash plus transcript round trip.
4. Bare haiku (H) halved the bill at equal quality.
5. The decision rule (S cheaper than A with quality at least A's on 2 of 4 tasks
   at n >= 3) is formally inconclusive at n = 1, but S lost on cost on all four
   tasks, so per-step nudging is demoted to opt-in.

## Routing eval results

| Configuration                         | Cases that delegated   | Delegations | Surface used                       | Mean cost per session |
| ------------------------------------- | ---------------------- | ----------- | ---------------------------------- | --------------------- |
| Pilot 1, all 4 configs (4 cases each) | 0 / 12 should-dispatch | 0           | none                               | $0.12                 |
| Pilot 2, Bash surface                 | 1 / 5                  | 2           | Bash dispatch                      | $0.25                 |
| Pilot 2, Bash surface + reminder      | 2 / 5                  | 5           | Bash dispatch                      | $0.31                 |
| Pilot 2, MCP tool available           | 1 / 5                  | 2           | Bash dispatch                      | $0.26                 |
| Pilot 2, MCP tool + reminder          | 2 / 5                  | 7           | MCP `delegate`, after a ToolSearch | $0.30                 |

What the transcripts settle:

- Pilot 1 was mostly the eval's fault. Three of its four should-dispatch cases
  were a single grep or a one-line edit, which the shipped routing table says to
  do directly. Six such cases are now negative controls.
- The reminder was injected on every turn it was enabled; the model still worked
  directly on most cases. When it did delegate it used whichever surface the
  reminder text named.
- The MCP tool had to be found through `ToolSearch` in the sessions that used
  it, so the reminder text steers, not tool visibility.
- Delegating sessions cost more ($0.54 to $0.73 on the multi-file rewrite)
  because the stub did no work and the model correctly redid it. Right
  fallback, wrong economics.

## What opencode costs

deepseek-v4-flash via opencode-go served every run today.

| Run                                                        | Tokens    | Cost    | Time            |
| ---------------------------------------------------------- | --------- | ------- | --------------- |
| Reply PONG (smoke)                                         | 17,407    | $0.0038 | 45 s            |
| Add a heading to a README (real edit)                      | -         | $0.0089 | 374 s           |
| Run a guarded command (approve guard fired under opencode) | 35,446    | $0.0042 | 47 s            |
| Benchmark tasks, whole task (4 runs)                       | 222,341   | $0.014  | 12 to 30 s each |
| Caveat-guard judge calls (8 runs)                          | 264,140   | $0.055  | 67 s median     |
| All 153 runs today                                         | 1,017,809 | $0.110  | -               |

## Recommendations, in order of measured value

1. **Route whole tasks to opencode, not steps.** `/cheap <task>`, `wf opencode
batch <glob> -t template.md`, `wf runbook run --cli opencode`, Telegram on
   its opencode backend. Measured: $0.370 to $0.014 across the four tasks.
2. **Inside a Claude session, delegate through the MCP `delegate` tool, never a
   Bash dispatch.** Point `clis/opencode-dispatch/rules/delegation.md` and the
   reminder text at the MCP tool first; the model uses what the text names.
   Measured: S $0.846 vs M $0.396.
3. **Leave per-step nudging off.** `/cheap-on` stays an explicit opt-in for a
   long mechanical batch. Measured: the reminder added $0.48 across four tasks
   and raised delegation from 1 to 2 cases of 5.
4. **Use haiku for well-specified execution.** A `--backend claude-haiku` on
   dispatch, or execution sessions on `--model haiku`, is the cheapest option
   that needs no provider key. Measured: $0.207 vs $0.370.
5. **Keep the fixed context small and gated.** The 71% cache-write cut is paid
   back on every turn. `root.agents-md-size` and `root.claude-plugins-consistent`
   enforce it; grow nested files and docs, never the root. Never delegate via
   the Agent tool (about $0.40 of cache write per spawn at the old size).
6. **Run the benchmark at n=3 before tuning further.**
   `node clis/agent-evals/bin/agent-evals.js delegation-benchmark
--runs=3 --conditions=A,S,M,W,H` (about $8 in Claude sessions, under $0.05
   in opencode). CSVs land under `.agents/tmp/delegation-benchmark/<timestamp>/`.

## Caveats

- One run per cell; identical runs vary by about 20%.
- Small, well-specified tasks. Larger or vaguer work may move the crossover,
  especially for W where no Claude reviews the result.
- The routing eval used a stub opencode; its costs include the model redoing
  skipped work.
- opencode prices are the provider's own on a capped subscription.
- All quality checks passed everywhere, so nothing here speaks to quality
  differences on harder tasks.

## Token diet (Phase K, 2026-09-02)

Where the money went in the session that ran this work (4,231 API calls, 524 turns): cache reads 81%,
cache writes 12%, output 7%, uncached input 0%. Median context per call 442 K tokens. A one-turn chat
session in this repo starts at about 50 K tokens of fixed prefix, so a 70-token answer costs about $0.07
on Sonnet. Full research and sources: `docs/solutions/workflow/token-diet-context-not-prose.md`.

### Ladder 1: single-turn cases (Sonnet, 12 chat prompts + 4 in-repo tasks, n=3, 384 sessions, $34.16, 0 errors)

Each arm adds one lever to the previous one. Chat rows are judged by an opencode rubric (required key
points minus forbidden claims, 0–1); agentic rows re-run the task's gate. `$/turn` is `total_cost_usd`.

| arm | lever added | chat $/turn | Δ vs T0 | chat out tok | judge | agentic $/task | Δ vs T0 | agentic out tok | gate |
|---|---|---|---|---|---|---|---|---|---|
| T0 | shipped config | $0.0886 | +0% | 582 | 0.83 | $0.1379 | +0% | 3966 | 3/3 |
| T1 | + output style | $0.0922 | +4% | 542 | 0.84 | $0.1319 | -4% | 3740 | 3/3 |
| T2 | + quiet hooks | $0.0951 | +7% | 542 | 0.80 | $0.1361 | -1% | 3481 | 3/3 |
| T3 | + prefix diet | $0.0647 | -27% | 421 | 0.80 | $0.1210 | -12% | 2226 | 3/3 |
| T4 | + squeez | $0.0721 | -19% | 479 | 0.83 | $0.1234 | -10% | 6153 | 3/3 |
| T5 | + autocompact 50% | $0.0698 | -21% | 501 | 0.80 | $0.1259 | -9% | 5173 | 3/3 |
| T6 | + effort medium | $0.0644 | -27% | 271 | 0.78 | $0.1241 | -10% | 2060 | 3/3 |
| T7 | + code diet | $0.0646 | -27% | 236 | 0.72 | $0.1121 | -19% | 2292 | 3/3 |

Reading, against the rule "keep a rung if total cost drops at least 5% with the judge within 0.05 of T0 and
the gate unchanged":

- **T1 output style**: cost flat (the +4% on chat is inside run-to-run noise on a $0.09 turn), output −7% on
  chat and −6% on agentic at equal judge, 🪨 on every reply. Fails the cost bar, kept for the reader: the
  point of the style is shorter replies, and the earlier standalone A/B on a status prompt measured 62 vs
  618 output tokens; heavier prompts compress less.
- **T2 quiet hooks**: no measurable cost change, judge −0.03. Kept for signal-to-noise (nothing in the
  agent's context on a healthy turn), not for money.
- **T3 prefix diet**: the one rung with real money in it, −27% per chat turn and −12% per agentic task,
  judge −0.03, gates 3/3. Ships: skill listing pruned per decision 2, CLAUDE.md 11.7 → 5.9 KB.
- **T4 squeez**: +11% on chat and +2% on agentic against T3, and agentic output tokens 2.8× higher (the
  model re-reads what the compressor cut). Fails, like rtk in JetBrains' measurement; not shipped, rtk not
  worth its own run.
- **T5 autocompact at 50%**: cannot show on single turns; judged by the chain run below.
- **T6 effort medium**: cost flat against T3 on both families, output −36% on chat, judge 0.78 (at the
  −0.05 boundary). Not a default; `/effort` stays a per-session choice.
- **T7 code diet**: agentic −7% against T3 (−19% against T0) at 3/3 gates, but chat judge 0.72 — the rules
  bled into prose answers in this cumulative arm. Re-measured alone as X7 (style + quiet hooks + prefix
  diet + code diet, no effort change) before deciding.

### Chains: does it hold over a whole conversation? (Sonnet, scripted turns replayed with `--resume`)

Whole-conversation `total_cost_usd`, mean of runs; 🪨 = turns ending with the canary
(`.agents/tmp/token-diet/ladder2-chains/rows.csv`, 326 turns, 0 errors):

| chain | T0 shipped | T1 soft style | T3 + quiet hooks + prefix diet | XH hard style + quiet hooks + prefix diet |
|---|---|---|---|---|
| chat-20 (n=2) | $0.707 | $0.420 (−41%) | — | — |
| agentic-15 (n=2) | $0.528 | $0.453 (−14%) | — | — |
| mixed-30 | $0.899 (n=2), 🪨 60/60 | $0.655 (−27%), 🪨 38/53 | $0.664 (−26%, n=1), 🪨 26/30 | **$0.612 (−32%, n=1), 🪨 30/30** |

Per-turn output tokens barely move (chat-20: 316 vs 304) while $/turn drops 0.035 → 0.021: over a
conversation the style saves calls, not words. The soft 5-line style loses its canary past ~20 turns
(8 of 23 on one mixed-30 run, issue #233's shape); the hard 3-line style keeps it on every turn.

### Hard style, single turns (X8 = hard style + quiet hooks + prefix diet; 21 of 48 sessions before a container restart)

chat $0.0650 / 400 output tokens / judge 0.78 (T3: $0.0647 / 421 / 0.80); agentic $0.111 (T3 $0.121),
gates passing. Equal cost to the soft-style rung, better canary over long chains: the hard style shipped.

### Shipped 2026-09-02 (`d7fc7b8b`)

Hard `token-diet` output style on through `.agents/config/claude-settings.json`; 26 skills removed (22 locked
including caveman, 4 ungated repo skills), `workflow-cli` gated to `frontend/**`; CLAUDE.md 11.7 → 5.9 KB;
caveman reminder hook, flag file and script removed; `agentation` MCP off by default. Not shipped: squeez
(decision 8), effort medium, code diet. Pending measurement on the shipped baseline (ladder 3): the Read
masker (`updatedToolOutput`, 800 B threshold) and rtk v0.47.0. Every decision with its evidence:
`docs/audits/2026-09-02-token-diet-decision-log.md`.

### Ladder 3: add-ons on the shipped baseline (Sonnet, 12 chat + 4 agentic, n=2, 77 sessions, $5.99, 0 new errors)

T0 here is the shipped config from `d7fc7b8b` (hard style, quiet hooks, prefix diet); each arm adds one hook.

| arm | lever | chat $/turn | Δ | chat out tok | judge | agentic $/task | Δ | gate |
|---|---|---|---|---|---|---|---|---|
| T0 | shipped config | $0.0731 | 0% | 262 | 0.73 | $0.1278 | 0% | 100% |
| X9 | + Bash/Read masker (`updatedToolOutput`, 800 B) | $0.0603 | −18% | 238 | 0.81 | $0.1100 | −14% | 100% |
| X10 | + rtk 0.47.0 rewrite hook | $0.0593 | −19% | 190 | 0.82 | $0.1231 | −4% | 100% |

Against the ladder-1 T0 ($0.0886 chat, $0.1379 agentic) the shipped baseline alone is −17% / −7%, and with
both hooks the chat turn is about −33%. Both hooks shipped (decision log 20–21): the masker as the new
`scripts/hooks/mask-large-output.mjs`, rtk's own hook script under `clis/env-bootstrap/hooks/` with an
`env-bootstrap` step that installs the pinned release into `~/.local/bin`. JetBrains' +7.6% for rtk did not
reproduce here; the agentic saving is small and the chat saving comes from the prompts that shell out.

## Benchmark B1: real savings, 7 tasks × 4 arms × n=3 (Sonnet vs Haiku vs in-session dispatch vs whole task on deepseek-v4-flash)

`.agents/tmp/delegation-benchmark/b1/results.csv`, 84 cells, 0 errors (16 cells re-run after the 2026-09-02 usage
outage with `--resume`). Cell = mean `total_cost_usd` / mean seconds / gate pass rate.

| task | size | lang | A Sonnet | H Haiku | S Sonnet + dispatch | W opencode only |
|---|---|---|---|---|---|---|
| add-validation-feature | medium | ts | $0.179 / 51s / 100% | $0.083 / 44s / 100% | $0.099 / 103s / 67% | $0.004 / 49s / 100% |
| multi-file-rename | small | ts | $0.097 / 21s / 100% | $0.047 / 27s / 100% | $0.211 / 146s / 100% | $0.004 / 37s / 100% |
| research-report | small | ts | $0.141 / 43s / 100% | $0.032 / 18s / 100% | $0.120 / 69s / 100% | $0.003 / 15s / 100% |
| index-generation | medium | ts | $0.097 / 20s / 100% | $0.038 / 22s / 100% | $0.165 / 214s / 100% | $0.004 / 29s / 100% |
| thrifty-01-wordfreq | small | js | $0.241 / 62s / 100% | $0.133 / 80s / 100% | $0.307 / 412s / 100% | $0.006 / 47s / 100% |
| thrifty-03-jqlite | medium | js | $0.472 / 144s / 100% | $0.289 / 186s / 100% | $0.275 / 102s / 67% | $0.026 / 181s / 100% |
| thrifty-07-taskgraph | large | go | $0.617 / 193s / 100% | $0.228 / 143s / 100% | $0.457 / 450s / 67% | $0.015 / 122s / 100% |
| **sum of task means** | | | **$1.844 / 535s** | **$0.850 / 520s** | **$1.635 / 1496s** | **$0.061 / 481s** |

Reading: the whole task on deepseek-v4-flash (W) passes every gate on all 7 tasks at 1/30 of Sonnet's cost and
10% less wall-clock in total (slower only on the two js greenfield tasks); Haiku halves cost at equal gates;
in-session dispatch (S) is a wash on cost, 2.8× slower and drops 3 gates — the eagerness/mechanics problem
Phase A measures. Ratios for `wf opencode savings`: `.agents/tmp/delegation-benchmark/b1/savings-ratios.json`.

## Eagerness sweep, stage 1b (partial, Sonnet, shipped token-diet config, 402 completed sessions, $104)

`.agents/tmp/delegation-eagerness/stage1b/rows.csv`. Positives = 11 routing-positive cases × runs; controls = 7
trivial cases that must stay direct. `first` = delegated on the first tool call; `any` = delegated at some
point; `ctrl FP` = controls that delegated anyway. Stopped by the usage window and then by cost.

| lever | n | positives | first | any | ctrl FP | mean $/session |
|---|---|---|---|---|---|---|
| L0 baseline | 72 | 33 | 0.00 | 0.12 | 0.09 | $0.281 |
| L1 skill | 72 | 33 | 0.06 | 0.24 | 0.12 | $0.298 |
| L2 rules file | 72 | 33 | 0.03 | 0.36 | 0.03 | $0.320 |
| **L3 reminder hook** | 72 | 33 | **0.36** | **0.76** | 0.06 | **$0.230** |
| L5 output style | 34 | 9 | 0.33 | 0.67 | 0.08 | $0.261 |
| L6 session agent | 28 | 6 | 0.00 | 0.50 | 0.05 | $0.205 |
| L7 soft gate | 27 | 6 | 0.00 | 0.50 | 0.00 | $0.189 |
| L8 prompt rewrite | 25 | 4 | 0.00 | 0.25 | 0.05 | $0.134 |
| L9 cost facts, L10 fork skill, L11 pre-dispatch | 0 | | | | | not reached |

Reading so far: the per-turn reminder (L3) is the only complete lever that moves eagerness (76% of positives
delegate at some point, 36% on the first call) and it is also the cheapest per session; the skill and the rules
file alone barely move it. The output style (L5) tracks L3 at a third of the sample. Nothing reaches the plan's
80% first-decision target yet; the untested levers L9–L11 and the L3+L5 combination are the remaining bets.

## cheap-flow tree, first live run (Phase C, 2026-09-03)

Feature: `cheap-flow check <slug>`, static validation of a plan's `units.jsonl` before the first paid dispatch (`docs/archive/plans/cf-check/`). Claude wrote `requirements.md`, `contract.md` and `units.jsonl` and read only the printed briefs; every research, implementation, review and documentation step ran on opencode project agents pinned to deepseek-v4-flash.

| step | agent | result | opencode USD | seconds |
| --- | --- | --- | --- | --- |
| research (4 questions) | researcher | 57-line brief with file:line evidence; found that `work` let `topoOrder` throws escape uncaught | 0.0078 | 48 |
| work u1 domain + unit tests | implementer | gate green, attempt 1 | 0.0052 | 100 |
| work u2 CLI subcommand + barrel | implementer | gate green, attempt 1 | 0.0067 | 58 |
| work u3 integration test | implementer | gate green, attempt 1 | 0.0061 | 99 |
| review | reviewer | 0 blockers, 2 risks (missing tests, both added), 3 nits (1 fixed, 2 dismissed) | 0.0412 (served by minimax after a deepseek timeout, see decision 27) | 572 |

Total opencode spend for the feature: about $0.07 and 15 minutes of agent time; Claude's part was three short documents and one adjudication. The run exposed one harness bug (the model rotation overriding an agent's model pin; fixed with `opencodeDispatch({ models })`) and one skill gap (no free validation of a hand-written plan; that is the feature itself, now called from `/cf-plan`). Decision log entries 26 and 27.

## Benchmark B2: arm T, the cheap-flow tree headless (2026-09-03, n=1, one task per size)

`.agents/tmp/delegation-benchmark/b2/results.csv`. Arm T = a Sonnet session writes `contract.md` + `units.jsonl`
(plan only, the task gate pinned as the last unit's gate), then `cheap-flow work` runs every unit on the opencode
implementer agent. Cut to one task per size at n=1 (decision 25); A is the B1 mean (n=3).

| task | size | A Sonnet alone | T tree: Claude plan + opencode | units | gate / success |
|---|---|---|---|---|---|
| multi-file-rename | small | $0.097 / 21s | **$0.257 + $0.006 / 132s** | 4 | success |
| add-validation-feature | medium | $0.179 / 51s | **$0.263 + $0.004 / 103s** | 1 | success |
| thrifty-07-taskgraph | large | $0.617 / 193s | **$0.260 + $0.021 / 255s** | 5 | `go test ./...` PASS, success |
| **sum** | | **$0.893 / 265s** | **$0.780 + $0.031 = $0.811 / 490s** | 10 | 3/3 |

Reading: the plan step costs about $0.26 of Sonnet regardless of task size (three planning sessions: $0.257, $0.263, $0.260), and the implementation costs $0.004–$0.021 of opencode. So the headless tree loses on the small (2.7× A) and medium (1.5× A) tasks and wins on the large one (−54%, $0.282 vs $0.617, gate green, 5 units). The crossover sits where Sonnet alone would spend more than about $0.30, i.e. the large bucket; below it, `W` (the whole task on opencode, $0.004–$0.026, 7/7 gates in B1) or Sonnet alone is cheaper. The decision rule (T adopted as the default flow if it saves ≥40% vs A on ≥5/7 tasks) is therefore not met for small and medium work; T is the right shape for large or multi-file work where the plan is worth a Sonnet turn, and for the interactive flow where the plan is written anyway. n=1; identical runs vary by about 20%.


## Ledger

The one-page reading of all of the above, generated from this report and the decision log: [Token Diet Ledger](https://claude.ai/code/artifact/ca5faf22-6c89-4dfb-af37-c23983636b56) (source `docs/audits/2026-09-03-token-diet-ledger.html`).
