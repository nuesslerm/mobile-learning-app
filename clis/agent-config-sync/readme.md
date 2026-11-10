# agent-config-sync

Renders the `.agents/` contract into each agent's native files: real files, never symlinks.

- Contract: `.agents/config/sources.json` (instructions dir, command/skill/rule sources, setup script) plus `.agents/config/{hooks,permissions,mcps,plugins,claude-settings}.json`, all zod-validated (`src/contracts/`).
- Outputs: `CLAUDE.md` + `AGENTS.md` per instruction area; Claude (`.claude/{settings.json,skills,commands,rules,setup.sh}`, `.mcp.json`); Codex (`.codex/{config.toml,hooks.json,execpolicy/project.rules}`, skills copied into `.agents/skills/`); opencode (`opencode.json`, `.opencode/{hooks.json,commands}`).
- `pnpm run sync:agent-config` writes; `pnpm run sync:check` exits 1 on drifted, stale or symlinked outputs.
- Golden outputs per tool: `tests/flow/golden/`, asserted by the flow tests (`cargo test --locked -p agent-config-sync --test flow`).
