---
title: Token diet — context size and call count decide the bill, prose compression decides readability
date: 2026-09-02
category: workflow
tags: [tokens, cost, output-style, hooks, caveman, compaction, squeez, rtk]
problem_type: knowledge
module: agent-config
---

## The map (this repo, one long remote session, 4,231 API calls over 524 turns)

| bucket | tokens | share of cost |
| --- | --- | --- |
| cache reads | 1,839 M | 81% |
| cache writes | 22.1 M | 12% |
| output (incl. thinking) | 3.15 M | 7% |
| uncached input | 0.2 M | 0% |

Median context per call 442 K tokens, p90 695 K; 74% of calls ran with more than 300 K in the window
(autocompact at 80% of 1 M). Tool results totalled 7.5 MB (Bash 3.9 MB, Read 2.0 MB) and were re-read on
every later call. A single one-turn chat session in this repo starts at about 50 K tokens of fixed prefix
(system prompt and tools cached, then CLAUDE.md, rules, 40 skill descriptions, hook output), so a 70-token
answer still costs about $0.07 on Sonnet.

So: making replies terse moves at most the 7%. Fewer tokens per call, fewer calls per turn, and a smaller
fixed prefix move the rest. Terseness still matters, for the reader.

## What independent benchmarks found (read 2026-09-02)

- JetBrains ran paired agentic benchmarks (80–86 tasks each, Harbor sandboxes) on the popular "token saver"
  add-ons: [caveman](https://blog.jetbrains.com/ai/2026/07/speak-to-ai-agents-like-cavemen-tosave-tokens/)
  −8.5% output tokens (advertised −65%), quality flat;
  [rtk](https://blog.jetbrains.com/ai/2026/07/rtk-claude-code-token-savings/) +7.6% cost at low effort, no
  change at high; [ponytail](https://blog.jetbrains.com/ai/2026/07/ponytail-skill-claude-tested/) (write
  less code) −10.3% cost, −15% code, −11% time, p=0.004, the only one that paid off.
- [Max Taylor](https://www.maxtaylor.me/articles/i-benchmarked-caveman-against-two-words): on 24 chat
  prompts, "Be brief." (419 tokens) ties caveman lite/full (401/404) and beats ultra (449); baseline 636.
  Caveman's real value is consistent output shape.
- Upstream [juliusbrussee/caveman](https://github.com/juliusbrussee/caveman): ultra is the top tier;
  [#233](https://github.com/JuliusBrussee/caveman/issues/233) ultra slips over long tool chains and after
  skill invocations; [#574](https://github.com/JuliusBrussee/caveman/issues/574) two compression hooks
  contradict each other; [#629](https://github.com/JuliusBrussee/caveman/issues/629) Fable-class harness
  prompts say "be selective about what you include, not fragments or arrows", so a per-turn ultra hook
  fights the system prompt every turn, and proposes selectivity compression instead (cut recaps, restated
  tool output, options not chosen, narration, hedging).
- Tool-output compressors: [rtk](https://github.com/rtk-ai/rtk) (PreToolUse rewrite of git/gh/test/lint
  output; Read and Grep untouched) and [squeez](https://github.com/claudioemmanuel/squeez) (PreToolUse on
  Bash plus PostToolUse `updatedToolOutput` on Read/Grep/Glob, cross-call dedup, zero runtime deps).
  `npm install -g squeez` runs its init from postinstall and writes hooks, a statusline and two slash
  commands into `~/.claude/` without asking; revert before measuring anything.
- Reviews: [dhung.dev](https://dhung.dev/blog/claude-code-token-optimizers) (do not stack two Layer-1 hooks),
  [firecrawl](https://www.firecrawl.dev/blog/claude-code-token-efficiency) (hidden baseline 20–30 K tokens,
  10–20 K per MCP server), [claude-token-efficient](https://github.com/drona23/claude-token-efficient)
  (63% headline, 4–12% on real work in its own fine print).
- Harness levers ([settings](https://code.claude.com/docs/en/settings)): `outputStyle` edits the system prompt
  itself (applies after `/clear`), `effortLevel` / `--effort` / `/effort`, `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`,
  path-scoped `.claude/rules/`, skill `paths:` gating, `--strict-mcp-config`.

## What this repo does about it

- **Output style, not a per-turn hook, carries the reply rules.** `.claude/output-styles/token-diet.md`
  (answer first; status or yes/no in 1–2 lines, else at most 5; cut content not grammar; the diff is the
  record; 🪨 canary) is switched on through `.agents/config/claude-settings.json` → `outputStyle`, so it is
  in the system prompt, survives compaction, keeps the cache prefix stable and can be turned off per session.
  A reply without 🪨 means the style is not in the prompt.
- **Hooks print nothing on a healthy turn.** `mise: ok`, `tailscale: ok`, the guard's "allowed" receipt were
  tokens with no reader; failures, blocks, judge outages and the dead-guard alarm still print.
- **Tool output goes on a diet too.** rtk's rewrite hook and a Bash|Read masker (`updatedToolOutput`, 800 B) were measured on the shipped baseline before either hook shipped (ladder 3: chat −19% / −18%, agentic −4% / −14%, judge 0.82 / 0.81 vs 0.73). squeez, measured the same way, cost more and stayed out.
- **The fixed prefix is the lever with real money in it**: skill listing pruned and `paths:`-gated,
  CLAUDE.md short, MCP servers off unless a task needs them (`--strict-mcp-config` in evals).
- **Measured before shipped.** `agent-evals token-diet` (since deleted) ran the arm ladder (T0 shipped → T1 style → T2
  quiet hooks → T3 prefix diet → T4 squeez/rtk → T5 compaction at 50% → T6 effort medium → T7 code diet) on
  real Sonnet sessions: 12 chat prompts with an opencode judge rubric, the in-repo benchmark tasks with their
  gate, and scripted 15–30 turn chains replayed into one session with `--resume` so decay past turn 1 is
  measured. Results: `docs/audits/2026-09-02-cheap-model-delegation-report.md`, section "Token diet".

## Rules that fell out

- A style rule the model must obey every turn belongs in the system prompt (output style), never in
  UserPromptSubmit `additionalContext`. The hook is for state the turn needs, and only when it changes.
- Judge conciseness on a rubric (required key points, forbidden claims), never on token count alone; a
  lever that drops a required fact loses.
- Score a tool-output compressor on `total_cost_usd` of a real task, never on its own byte-count dashboard.
- Never install a token tool with `npm install -g` into a machine whose `~/.claude` you have not backed up.
