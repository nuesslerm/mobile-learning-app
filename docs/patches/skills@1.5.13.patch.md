# skills Patch

Two fixes applied to `dist/cli.mjs`.

**1. YAML `": "` in descriptions causes silent skill drop (`parseFrontmatter`)**

The CLI uses the `yaml` package to parse `SKILL.md` frontmatter. YAML treats an unquoted value
containing `": "` as a nested mapping — e.g.
`description: improve context efficiency: reduce tokens` is parsed as
`{ description: { "improve context efficiency": "reduce tokens" } }` rather than a plain string.

`parseSkillMd` checks `typeof data.description !== "string"` and silently returns `null` on mismatch,
so any skill whose description contains `": "` is invisibly dropped from discovery. This affected
`context-optimization`, `harness-engineering`, `filesystem-context`, `tool-design`, and others in the
`muratcankoylan/agent-skills-for-context-engineering` collection.

The fix pre-processes the raw frontmatter string before YAML parsing: unquoted scalar values
containing `": "` are wrapped in double-quotes (with any inner quotes escaped).

**2. `experimental_install` (lock file restore) ignores `--full-depth` (`runInstallFromLock`)**

`experimental_install` reads `skills-lock.json` and calls `runAdd` internally to reinstall each
source. It did not pass `fullDepth: true`, so repos with a root `SKILL.md` (like the
`muratcankoylan` collection) hit the early-return discovery path and failed to find individual skills
— making `pnpm run skills:install` require listing every skill name explicitly.

The fix passes `fullDepth: true` in the `runAdd` call inside `runInstallFromLock`, so
`skills experimental_install` correctly restores all skills recorded in `skills-lock.json`.
