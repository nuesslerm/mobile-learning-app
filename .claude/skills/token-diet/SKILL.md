---
name: token-diet
description: Make the active Claude session cheaper: the token-diet output style (answer first, hard caps, the diff is the record, 🪨 canary), quiet hooks, a small fixed prefix. Use for /token-diet on|off|status, "be brief", "less tokens", "caveman" (replaced by this), or when replies or context feel bloated.
---

# token-diet

Where a session's money goes (measured on this repo, 2026-09-02): cache reads 81%, cache writes 12%, output 7%. Prose compression alone moves the 7%. The diet works the other 93% too.

## What is on

- **Output style** `.claude/output-styles/token-diet.md`, set through `.agents/config/claude-settings.json` (`outputStyle`). At most 3 lines, bullets, answer in the first line; status, yes/no and "what changed" in 1 line; no code unless asked; after edits the file list and one word for the outcome; end every reply with 🪨. Measured 2026-09-02: −32% whole-conversation cost on a 30-turn chain and 🪨 on 30/30 turns where the softer 5-line style decayed. The 🪨 is the drift canary: a reply without it means the style fell out of the system prompt (check `outputStyle` after `/clear` or a settings sync).
- **Quiet hooks**: every per-turn hook prints nothing when the machine is healthy. Status lines on a good turn are tokens with no reader.
- **Small fixed prefix**: skills gated by `paths:` where they are area-specific, CLAUDE.md kept short, MCP servers off unless a task needs them.
- **Tool output on a diet**: `rtk` (PreToolUse, rewrites git/test/lint/package-manager commands to its compact forms; installed by `.claude/setup.sh`, pinned) and the Bash|Read masker (PostToolUse `updatedToolOutput`: results over 800 B enter context as a 40-line head plus a pointer to the full text under `.agents/tmp/`; Read the file with offset/limit when more is needed). Measured 2026-09-03: chat −19% / −18%, agentic −4% / −14%, judge up.

## Commands

| ask | do |
| --- | --- |
| `/token-diet status` | read `.claude/settings.json` `outputStyle` and `.agents/config/claude-settings.json`; report on/off and whether the last reply carried 🪨 |
| `/token-diet off` | remove `outputStyle` from `.agents/config/claude-settings.json`, run `pnpm run sync:agent-config`; the change applies after `/clear` or a restart. Per session only: `/output-style default` |
| `/token-diet on` | set `"outputStyle": "token-diet"` in `.agents/config/claude-settings.json`, run `pnpm run sync:agent-config` |
| `/effort high` | when a task needs deeper reasoning; the default here is the model default |

Never edit `.claude/settings.json` directly; it is generated.

## Measuring

`wf offload ask --stats` and `wf offload ask --savings` price delegation; the style itself was measured once against real sessions. Numbers and sources: `docs/audits/2026-09-02-cheap-model-delegation-report.md`, section "Token diet".
