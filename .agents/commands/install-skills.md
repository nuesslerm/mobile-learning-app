---
description: Install, restore, list, update, and remove agent skills; explains skills-lock.json and the local-edit overwrite boundary.
allowed-tools: Bash, Read, Edit, Write, Glob, Grep
---

# Installing and managing skills

This repo installs third-party agent skills under `.agents/skills/`, mirrored by
the `skills` CLI into per-agent dirs (`.claude/skills/`, `.codex/skills/`,
`.gemini/skills/`, …). `skills-lock.json` is the source of truth for **which
skills are installed, where they came from, and what upstream hash they
correspond to**.

## Install one skill

```bash
npx skills add <owner/repo> --full-depth --skill '<skill-name>' --agent '*' -y
```

## Install all skills from a repo

```bash
npx skills add <owner/repo> --full-depth --all
```

## Restore from the lock file

On a clean checkout (or whenever you want to re-sync to `skills-lock.json`,
e.g. after pulling main):

```bash
pnpm run skills:install   # alias for: skills experimental_install
```

## Update

Refetch the upstream copy of a single skill (or every locked skill):

```bash
npx skills update <skill-name>        # one
npx skills update --all -y            # all locked, no prompt
```

See the **Lock-file contract** section below before running either of these —
they overwrite any local edits you made to the installed file.

## List

```bash
npx skills list
```

## Remove a skill

```bash
npx skills remove <skill-name>        # one
npx skills remove <a> <b> <c> -y      # several, no prompt
```

`skills remove` deletes the local copies under every agent dir but does
**not** prune `skills-lock.json`. Remove the matching entries from the lock by
hand (keep the JSON valid), then re-verify with `npx skills list`.

## Lock-file contract

`skills-lock.json` records each installed skill's upstream `source` +
`skillPath` and a `computedHash` of the **upstream** content — not of any local
edits. `pnpm run skills:install` and `npx skills update …` both restore /
overwrite local copies from those upstream entries.

**Any local edits to an installed skill will be overwritten** the next time
restore/update runs while the lock entry still points upstream. Only run
restore/update when intentionally refreshing from upstream; afterward,
reapply and review any repo-specific adaptations you had made.

## After changing agent commands or config

Commands in `.agents/commands/`, plus the agent config files under
`.agents/config/` (hooks, permissions, plugins), are mirrored into per-agent
generated files (`.claude/settings.json`, `.opencode/hooks.json`, …). After
editing any of those sources:

```bash
pnpm run sync:agent-config
```

A pre-commit gate (`sync:check`) rejects commits where the generated files
diverge from the canonical sources.

## Example: a few skills from a multi-skill plugin repo

Most plugins ship many skills; pick the ones you want by name rather than
pulling everything (this repo removed its last 22 locked skills on
2026-09-02 for the token diet: every skill description is paid for on every
turn, see `docs/audits/2026-09-02-token-diet-decision-log.md`):

```bash
npx skills add <owner/repo> --full-depth \
  --skill <skill-a> --skill <skill-b> \
  --agent '*' -y
```

Lock the result with `pnpm run skills:install` and verify with
`npx skills list`.
