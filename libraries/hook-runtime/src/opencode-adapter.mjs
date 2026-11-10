/**
 * opencode plugin adapter for a Claude-Code-style `hooks.json`.
 *
 * `agent-config-sync` renders the canonical `.agents/config/hooks.json` into
 * `.opencode/hooks.json` with opencode's event names (`session.created`,
 * `session.deleted`, `tool.execute.before`, `tool.execute.after`). This module
 * turns that file into an opencode plugin so the SAME hook binaries run under
 * opencode as under Claude Code -- with the same stdin contract and the same
 * exit-code semantics from `./index.mjs`:
 *
 *   exit 0  proceed; a stdout `hookSpecificOutput.permissionDecision: "deny"`
 *           still blocks (that is how Claude Code hooks deny), by throwing
 *   exit 2  block: the hook's stderr becomes the thrown error's message
 *   exit 1  the hook itself failed: logged to stderr, never blocks
 *
 * The hand-rolled adapter this replaces ran each command with stdio inherited
 * and NO stdin payload, so a hook that reads `tool_input` from stdin (the
 * approve guard) saw an empty object and silently allowed everything; its
 * matcher was also a substring match, so `Bash` matched `bash_output`.
 *
 * Plain ESM, no dependencies: opencode loads it straight from `.opencode/plugin/`.
 */
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';

/** opencode tool names -> the Claude Code tool names hooks are written against. */
const TOOL_NAME_ALIASES = {
  bash: 'Bash',
  edit: 'Edit',
  write: 'Write',
  read: 'Read',
  glob: 'Glob',
  grep: 'Grep',
  webfetch: 'WebFetch',
  websearch: 'WebSearch',
  task: 'Agent',
};

export function normalizeToolName(name) {
  if (typeof name !== 'string' || name.length === 0) return '';
  return TOOL_NAME_ALIASES[name.toLowerCase()] ?? name;
}

/** Claude Code matcher semantics: anchored regex over the tool name; empty or `*` matches all. Case-insensitive so `Bash` matches opencode's `bash`. */
export function matcherMatches(matcher, toolName) {
  if (!matcher || matcher === '*') return true;
  try {
    return new RegExp(`^(?:${matcher})$`, 'i').test(toolName);
  } catch {
    return matcher.toLowerCase() === toolName.toLowerCase();
  }
}

/** Shape opencode's tool args like Claude Code's `tool_input` so a hook written for one runs under the other. */
export function toToolInput(toolName, args) {
  const a = args && typeof args === 'object' ? args : {};
  const out = { ...a };
  if (toolName === 'Bash' && typeof a.command === 'string')
    out.command = a.command;
  const filePath = a.filePath ?? a.file_path ?? a.path;
  if (
    (toolName === 'Edit' || toolName === 'Write' || toolName === 'Read') &&
    typeof filePath === 'string'
  ) {
    out.file_path = filePath;
  }
  if (toolName === 'Edit') {
    if (typeof a.oldString === 'string') out.old_string = a.oldString;
    if (typeof a.newString === 'string') out.new_string = a.newString;
  }
  return out;
}

function parseDecision(stdout) {
  const trimmed = (stdout ?? '').trim();
  if (!trimmed) return null;
  try {
    const parsed = JSON.parse(trimmed);
    const inner = parsed && parsed.hookSpecificOutput;
    if (!inner || typeof inner !== 'object') return null;
    return {
      permissionDecision: inner.permissionDecision,
      reason: inner.permissionDecisionReason,
      additionalContext: inner.additionalContext,
    };
  } catch {
    return null;
  }
}

function loadHooksFile(hooksFile) {
  if (!existsSync(hooksFile)) return {};
  const parsed = JSON.parse(readFileSync(hooksFile, 'utf8'));
  return (parsed && parsed.hooks) || {};
}

/**
 * Runs every hook registered for `event` whose matcher accepts `toolName`.
 * Returns the per-hook results; throws on the first block so opencode
 * reports it to the model. `run` is injectable for tests.
 */
export function runHooksFor({
  hooks,
  event,
  toolName = '',
  payload,
  cwd,
  run = spawnSync,
  log = console.error,
  timeoutMs = 120_000,
}) {
  const results = [];
  for (const block of hooks[event] ?? []) {
    if (block.matcher && !matcherMatches(block.matcher, toolName)) continue;
    for (const hook of block.hooks ?? []) {
      if (hook.type !== 'command' || typeof hook.command !== 'string') continue;
      const r = run('sh', ['-c', hook.command], {
        cwd,
        input: JSON.stringify(payload),
        encoding: 'utf8',
        env: {
          ...process.env,
          CLAUDE_PROJECT_DIR: cwd,
          OPENCODE_HOOK_EVENT: event,
        },
        timeout: hook.timeout ? hook.timeout * 1000 : timeoutMs,
      });
      const result = {
        command: hook.command,
        exitCode: r.status,
        stdout: r.stdout ?? '',
        stderr: r.stderr ?? '',
      };
      results.push(result);
      if (r.status === 2) {
        throw new Error(
          (r.stderr || r.stdout || `hook blocked: ${hook.command}`).trim()
        );
      }
      if (r.status === 0) {
        const decision = parseDecision(r.stdout);
        if (decision && decision.permissionDecision === 'deny') {
          throw new Error(
            (decision.reason || `hook denied: ${hook.command}`).trim()
          );
        }
        continue;
      }
      // exit 1 (or a spawn failure): loud, non-blocking -- same posture as failLoud.
      log(
        `[hooks] ${event} hook failed (exit ${r.status ?? 'spawn error'}): ${hook.command}\n${(r.stderr || '').trim()}`
      );
    }
  }
  return results;
}

/**
 * The opencode plugin. `hooksFile` is the synced `.opencode/hooks.json`;
 * `projectRoot` defaults to the directory opencode hands the plugin.
 */
export function createOpencodeHooksPlugin({
  hooksFile,
  projectRoot,
  run,
  log,
} = {}) {
  return async ({ directory }) => {
    const cwd = projectRoot ?? directory ?? process.cwd();
    const hooks = loadHooksFile(hooksFile);
    const base = (extra) => ({
      cwd,
      hook_event_name: extra.hook_event_name,
      ...extra,
    });
    // session.created has no callback in the plugin API: it IS plugin load.
    runHooksFor({
      hooks,
      event: 'session.created',
      payload: base({ hook_event_name: 'SessionStart' }),
      cwd,
      run,
      log,
    });

    return {
      'session.deleted': async (input) => {
        runHooksFor({
          hooks,
          event: 'session.deleted',
          payload: base({
            hook_event_name: 'Stop',
            session_id: input?.sessionID ?? input?.info?.id,
          }),
          cwd,
          run,
          log,
        });
      },
      'tool.execute.before': async (input, output) => {
        const toolName = normalizeToolName(input?.tool);
        runHooksFor({
          hooks,
          event: 'tool.execute.before',
          toolName,
          payload: base({
            hook_event_name: 'PreToolUse',
            session_id: input?.sessionID,
            tool_name: toolName,
            tool_input: toToolInput(toolName, output?.args),
          }),
          cwd,
          run,
          log,
        });
      },
      'tool.execute.after': async (input, output) => {
        const toolName = normalizeToolName(input?.tool);
        runHooksFor({
          hooks,
          event: 'tool.execute.after',
          toolName,
          payload: base({
            hook_event_name: 'PostToolUse',
            session_id: input?.sessionID,
            tool_name: toolName,
            tool_input: toToolInput(toolName, input?.args),
            tool_response: output?.output ?? output,
          }),
          cwd,
          run,
          log,
        });
      },
    };
  };
}
