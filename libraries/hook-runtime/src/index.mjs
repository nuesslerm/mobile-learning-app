/**
 * Shared runtime for every agent hook in this repo.
 *
 * WHY THIS EXISTS. Nine hooks were written independently and all nine made the
 * same mistake: they caught their own internal errors and exited 0. Claude Code
 * routes an exit-0 hook's stderr to the debug log ONLY, never the transcript,
 * so a permanently broken guard and a guard that keeps passing produce
 * identical evidence from inside a session. `agent-approve-guard` has this in
 * its own history — a swallowed ENXIO silently disabled every guard rule for
 * every Bash/Edit/Write call, with nothing surfaced anywhere.
 *
 * Fixing those nine by hand left six near-copies of the same `failLoud` helper,
 * which is the same bug waiting for hook number ten. This module is the one
 * implementation; a new hook must import it rather than hand-roll it again.
 *
 * Plain ESM with no dependencies and no build step, because hooks run in the
 * worst environment available: before `pnpm install` has created node_modules,
 * from a committed bundle, and inside sandboxes with unusual stdin plumbing.
 */

/**
 * The three things a hook can mean. Anything else is a bug in the hook.
 *
 * PROCEED (0)       this input is not our business — silent, correct
 * NOT_ENFORCING (1) the hook could not do its job — LOUD, non-blocking
 * BLOCK (2)         a policy decision — blocking, message shown to the agent
 *
 * NOT_ENFORCING is 1 rather than 2 deliberately: hooks gate every tool call and
 * every turn end, so blocking on a hook's own bug bricks the session — a
 * lockout this repo has already had to recover from. Visible-but-non-blocking
 * is the right trade. Invisible is not a trade, it is the bug.
 */
export const HOOK_EXIT = {
  PROCEED: 0,
  NOT_ENFORCING: 1,
  BLOCK: 2,
};

/**
 * The hook could not do its job. Names the hook and the failure on stderr, then
 * exits 1 so Claude Code prints a `hook error` notice in the transcript.
 *
 * Never call this for a policy decision — that is `block`.
 */
export function failLoud(hookName, what, error) {
  const detail =
    error === undefined
      ? ''
      : `: ${error instanceof Error ? error.message : String(error)}`;
  process.stderr.write(`${hookName} NOT ENFORCING — ${what}${detail}\n`);
  process.exit(HOOK_EXIT.NOT_ENFORCING);
}

/** This input is not this hook's business. Silent and correct. */
export function proceed() {
  process.exit(HOOK_EXIT.PROCEED);
}

/**
 * A policy decision: refuse the action and tell the agent why. The message is
 * the agent's only explanation, so it should say what to do instead.
 */
export function block(message) {
  process.stderr.write(`${message}\n`);
  process.exit(HOOK_EXIT.BLOCK);
}

/**
 * Reads and parses the hook payload from stdin.
 *
 * Reads fd 0 rather than '/dev/stdin': a sandboxed container can back a piped
 * stdin with an anonymous pipe that has no /dev/stdin device node, so
 * readFileSync on the path throws ENXIO while the pipe itself has data ready.
 * That exact failure silently disabled agent-approve-guard once.
 *
 * A read or parse failure is NOT_ENFORCING, never PROCEED: Claude Code always
 * sends valid JSON, so failing here means the hook is not guarding and that has
 * to be visible.
 */
export function readHookInput(hookName) {
  let raw;
  try {
    raw = readStdinSync();
  } catch (error) {
    failLoud(hookName, 'could not read hook input from stdin', error);
  }

  try {
    const parsed = JSON.parse(raw);
    if (parsed === null || typeof parsed !== 'object') {
      failLoud(hookName, `hook input was not a JSON object (got ${typeof parsed})`);
    }
    return parsed;
  } catch (error) {
    if (error instanceof SyntaxError) {
      failLoud(hookName, 'could not parse hook input as JSON', error);
    }
    throw error;
  }
}

function readStdinSync() {
  // Imported lazily so this module stays importable in environments where a
  // top-level node:fs import would be the only thing that fails.
  const { readFileSync } = require_('node:fs');
  return readFileSync(0, 'utf-8');
}

// createRequire rather than a static import so this file has no import-time
// side effects at all — a hook that fails to even load is the one failure this
// module cannot report on.
function require_(specifier) {
  return globalThis.process.getBuiltinModule
    ? globalThis.process.getBuiltinModule(specifier)
    : // eslint-disable-next-line no-eval -- fallback for runtimes without getBuiltinModule
      eval('require')(specifier);
}

/**
 * Runs a hook body so that ANY escaping error becomes a loud NOT_ENFORCING exit
 * instead of a silent success.
 *
 * This is the part that makes the original bug unwritable: a hook using
 * `runHook` cannot accidentally exit 0 on an internal failure, because the only
 * paths out are `proceed`, `block`, or this catch.
 */
export async function runHook(hookName, body) {
  try {
    await body();
  } catch (error) {
    failLoud(hookName, 'threw while running', error);
  }
}
