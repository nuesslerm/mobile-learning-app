# agent-approve-guard/src

## Responsibility
Core implementation of the guard system — CLI, plugin, config loading, parsing, and approval flow.

## Design
- **Bin**: `agent-approve-guard.ts` — CLI + Claude Code hook interface
- **OpenCode**: `plugin.ts` — MCP plugin server
- **Config**: `loader.ts` — glob search for `.agent-approve-guard*.json`, merge by priority; `validator.ts` — schema validation
- **Parser**: `shell.ts` — `splitShellCommands`, `shlexSplit`, `stripWrappers`; `analyze.ts` — command analysis; `match.ts` — guard pattern matching
- **Guard**: `approval.ts` — approval flow loop; `predicateRunner.ts` — condition script execution; `contextRunner.ts` — context script → KEY=value map

## Flow
CLI input (stdin JSON or OpenCode tool call) → shell parser → guard matcher → predicate evaluation → approval flow or block

## Integration
- Depends on: Node.js builtins (fs, path, crypto, child_process)
- Consumed by: `src/bin/` entry points