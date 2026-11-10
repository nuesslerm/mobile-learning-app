#!/usr/bin/env node
// `condition` script for `libraries/hook-runtime/src/deny.json` -- closes the leak
// class documented in docs/solutions/security/wf-secrets-default-backend-and-redaction-pitfalls.md
// and docs/solutions/security/sync-env-stdout-redaction.md, which has now
// fired at least three times in this repo's history: `wf secret get`/
// `bws secret get|list` print a real secret value to stdout BY DESIGN (the
// `$(wf secret get X)` capture pattern is this repo's primary safe usage),
// but nothing stopped that value from being redirected to a file or piped
// into a content-dump command in the same breath -- which is exactly how
// BWS_DEPLOY_SECRETS_EDIT_ACCESS_TOKEN leaked into an agent's own tool
// output this session (`wf secret get ... >/tmp/x.txt`, then `head -c 40
// /tmp/x.txt` two commands later in the same Bash call).
//
// Fires (exit 0 => guard blocks) only when the fetched VALUE itself reaches a leak-shaped sink, so a fetch may share a
// Bash call with unrelated commands (a `head` of another file, a `curl ... | head` of a response). The command is split
// into list items (`; && || newline`) and pipeline stages (`|`), outside quotes and nested `$( )`, `<( )`, `( )` and
// backticks, and a stage's output counts as carrying a secret when it is
//   - a fetch itself (`wf secret get|env`, `bws secret get|list`, `security find-generic-password`),
//   - an `echo`/`printf` of a shell variable that was assigned from a fetch, or of a substituted fetch, or
//   - a parenthesised group that prints one.
// A secret-carrying pipeline is blocked when a later stage is a dump word, `tee`, `xargs` or a network-egress tool, or
// when any of its stages redirects to a real file. Independently, a dump word or `tee` handed the secret (variable,
// here-string, process substitution) is blocked, and so is an egress tool handed it as a here-string. USING a captured
// secret in a header or argument of an egress tool is this repo's own documented pattern
// (`TOK=$(wf secret get X); curl -H "Bearer $TOK" ...`) and stays allowed.
// `eval "$(wf secret env)"` loads the whole credential set into the environment, so there every secret-named variable
// counts as captured and environment listings (`printenv`, bare `env`/`set`, `export -p`, ...) are blocked.
//
// A command this scanner cannot parse (unbalanced quote or parenthesis) falls back to the old coarse rule: a secret
// source plus any dump word or file redirect anywhere in the string, or a source piped straight into an egress tool.
// The false-negative cost of this guard is a live credential in a transcript, so it errs toward blocking when unsure.
//
// Receives the full original command string via GUARD_COMMAND (see
// libraries/hook-runtime/src/guard.mjs runCondition) -- not just the
// segment that matched -- so it sees the whole picture regardless of which
// segment triggered the guard.

const command = process.env.GUARD_COMMAND ?? '';

// The `wf secret` subcommands that print a real value unconditionally. This script stays zero-dependency so it works on the
// PreToolUse critical path, so the list is a deliberate, independent copy of the one in
// clis/secrets/tests/flow/secrets.rs (the_value_revealing_..., pre-commit `secrets` job), which
// enforces that the two stay in sync with libraries/hook-runtime/src/deny.json -- update all three together.
const WF_SECRETS_VALUE_REVEALING_SUBCOMMANDS = ['get', 'env'];
const WF_SECRETS_SOURCE_FRAGMENT = String.raw`wf\s+secret\s+(?:${WF_SECRETS_VALUE_REVEALING_SUBCOMMANDS.join('|')})`;
const SOURCE_FRAGMENT = String.raw`(?:${WF_SECRETS_SOURCE_FRAGMENT}|bws\s+secret\s+(?:get|list)|security\s+find-generic-password)\b`;
const SOURCE_AT_START_RE = new RegExp(`^${SOURCE_FRAGMENT}`);
const SOURCE_ANYWHERE_RE = new RegExp(String.raw`\b${SOURCE_FRAGMENT}`);

const DUMP_WORDS = new Set(['head', 'cat', 'tail', 'more', 'less', 'strings', 'xxd', 'od', 'hexdump', 'nl', 'tac', 'base64']);
const EGRESS_WORDS = new Set(['curl', 'wget', 'nc', 'ncat', 'netcat', 'ssh', 'scp', 'rsync', 'ftp', 'sftp', 'socat']);
const PIPE_SINKS = new Set([...DUMP_WORDS, ...EGRESS_WORDS, 'tee', 'xargs']);
const SHELL_WRAPPERS = new Set(['bash', 'sh', 'zsh', 'dash', 'eval']);
const PRINTERS = new Set(['echo', 'printf']);

// Excludes /dev/null (the safe-discard target) and &<digit> (fd duplication
// like `2>&1`, which redirects to another stream, never to a real file).
const FILE_REDIRECT_RE = />>?\s*(?!\/dev\/null)(?!&\d)\S/;
const HERE_STRING_RE = /<<</;

const ENV_FETCH_RE = /\bwf\s+secret\s+env\b/;
const ENV_SECRET_NAME_RE = /SECRET|TOKEN|KEY|SEED|PASSWORD|PASSWD|CREDENTIAL|AUTH/;
const ENV_LISTING_RE =
  /(?:^|[\s;&|(])(?:printenv|export\s+-p|declare\s+-[A-Za-z]*[px]|compgen\s+-e)(?=$|[\s;&|)])|(?:^|[;&|(]\s*)(?:env|set)\s*(?:$|[;&|)<>])|\/proc\/[^\s/]+\/environ/;

const MARK = '\u0001';
// Words that may precede the real command word: declaration builtins, wrappers and `NAME=value` assignments.
const LEAD_RE =
  /^\s*(?:(?:export|local|declare|readonly|typeset|time|command|nohup|exec|sudo|env|builtin)(?:\s+-\S+)*\s+|[A-Za-z_]\w*\+?=\S*\s+)*/;

class Unbalanced extends Error {}

function skipSingle(text, i) {
  const j = text.indexOf("'", i + 1);
  if (j < 0) throw new Unbalanced();
  return j;
}

function skipBacktick(text, i) {
  for (let k = i + 1; k < text.length; k++) {
    if (text[k] === '\\') k++;
    else if (text[k] === '`') return k;
  }
  throw new Unbalanced();
}

function skipDouble(text, i) {
  for (let k = i + 1; k < text.length; k++) {
    const c = text[k];
    if (c === '\\') k++;
    else if (c === '"') return k;
    else if (c === '$' && text[k + 1] === '(') k = skipParen(text, k + 1);
    else if (c === '`') k = skipBacktick(text, k);
  }
  throw new Unbalanced();
}

function skipParen(text, i) {
  let depth = 0;
  for (let k = i; k < text.length; k++) {
    const c = text[k];
    if (c === '\\') k++;
    else if (c === "'") k = skipSingle(text, k);
    else if (c === '"') k = skipDouble(text, k);
    else if (c === '`') k = skipBacktick(text, k);
    else if (c === '(') depth++;
    else if (c === ')' && --depth === 0) return k;
  }
  throw new Unbalanced();
}

// Splits at separators that sit outside quotes, backticks and parentheses; `separatorAt` returns the separator length.
function split(text, separatorAt) {
  const parts = [];
  let start = 0;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === '\\') i++;
    else if (c === "'") i = skipSingle(text, i);
    else if (c === '"') i = skipDouble(text, i);
    else if (c === '`') i = skipBacktick(text, i);
    else if (c === '(') i = skipParen(text, i);
    else {
      const n = separatorAt(text, i);
      if (n) {
        parts.push(text.slice(start, i));
        i += n - 1;
        start = i + 1;
      }
    }
  }
  parts.push(text.slice(start));
  return parts;
}

const listSeparator = (t, i) => {
  const c = t[i];
  if (c === ';' || c === '\n') return 1;
  if ((c === '&' && t[i + 1] === '&') || (c === '|' && t[i + 1] === '|')) return 2;
  if (c === '&' && !'<>&'.includes(t[i - 1] ?? '') && t[i + 1] !== '>') return 1;
  return 0;
};
const pipeSeparator = (t, i) => (t[i] === '|' ? 1 : 0);

const envLoaded = ENV_FETCH_RE.test(command);

function isSecretVariable(name, vars) {
  return vars.has(name) || (envLoaded && ENV_SECRET_NAME_RE.test(name));
}

// Rewrites one stage into a flat string: quoted text and substitution bodies are replaced by placeholders (MARK when the
// piece carries a secret), so words, redirects and here-strings can be matched without being fooled by quoted text.
// Substitution bodies are analysed on their own and a leak inside one marks the whole command.
function flatten(text, vars, state, inDouble = false) {
  let flat = '';
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === '\\') {
      flat += '_';
      i++;
    } else if (!inDouble && c === "'") {
      const j = skipSingle(text, i);
      state.literals.push(text.slice(i + 1, j));
      flat += "'_'";
      i = j;
    } else if (!inDouble && c === '"') {
      const j = skipDouble(text, i);
      const inner = text.slice(i + 1, j);
      state.literals.push(inner);
      const piece = flatten(inner, vars, state, true);
      flat += `"${piece.includes(MARK) ? MARK : '_'}"`;
      i = j;
    } else if (c === '`') {
      const j = skipBacktick(text, i);
      flat += substitution(text.slice(i + 1, j), vars, state) ? MARK : '_';
      i = j;
    } else if (c === '$' && text[i + 1] === '(') {
      const j = skipParen(text, i + 1);
      flat += substitution(text.slice(i + 2, j), vars, state) ? MARK : '_';
      i = j;
    } else if (!inDouble && (c === '<' || c === '>') && text[i + 1] === '(') {
      const j = skipParen(text, i + 1);
      flat += substitution(text.slice(i + 2, j), vars, state) ? MARK : '_';
      i = j;
    } else if (!inDouble && c === '(') {
      const j = skipParen(text, i);
      if (substitution(text.slice(i + 1, j), vars, state)) state.groupOut = true;
      flat += '_';
      i = j;
    } else if (c === '$') {
      const ref = /^\$(?:\{(\w+)[^}]*\}|(\w+))/.exec(text.slice(i));
      if (ref && isSecretVariable(ref[1] ?? ref[2], vars)) {
        flat += MARK;
        i += ref[0].length - 1;
      } else {
        flat += c;
      }
    } else {
      flat += c;
    }
  }
  return flat;
}

// Analyses a substitution or group body; true when its output carries a secret.
function substitution(body, vars, state) {
  const result = analyze(body, vars);
  if (result.leak) state.leak = true;
  return result.out;
}

function inspect(stage, vars) {
  const state = { literals: [], groupOut: false, leak: false };
  const flat = flatten(stage, vars, state);
  const lead = LEAD_RE.exec(`${flat} `)[0];
  for (const [, name, value] of lead.matchAll(/([A-Za-z_]\w*)\+?=(\S*)/g)) {
    if (value.includes(MARK)) vars.add(name);
  }
  const rest = `${flat} `.slice(lead.length).trim();
  const word = (rest.split(/\s+/)[0] ?? '').replace(/^\\/, '').split('/').pop();
  const secret = flat.includes(MARK);
  if (word === 'read' && secret) {
    for (const token of rest.split(/\s+/).slice(1)) {
      if (/^[A-Za-z_]\w*$/.test(token)) vars.add(token);
    }
  }
  let fetch = SOURCE_AT_START_RE.test(rest);
  if (SHELL_WRAPPERS.has(word)) {
    for (const literal of state.literals) {
      if (!SOURCE_ANYWHERE_RE.test(literal)) continue;
      fetch = true;
      if (analyze(literal, vars).leak) state.leak = true;
    }
  }
  return { flat, word, secret, fetch, groupOut: state.groupOut, leak: state.leak };
}

function analyze(text, vars) {
  let leak = false;
  let out = false;
  for (const segment of split(text, listSeparator)) {
    let flowing = false;
    for (const stage of split(segment, pipeSeparator)) {
      const s = inspect(stage, vars);
      if (s.leak) leak = true;
      const sink = PIPE_SINKS.has(s.word);
      const redirects = FILE_REDIRECT_RE.test(s.flat);
      if (flowing && (sink || redirects)) leak = true;
      if (s.secret && (DUMP_WORDS.has(s.word) || s.word === 'tee')) leak = true;
      if (s.secret && EGRESS_WORDS.has(s.word) && HERE_STRING_RE.test(s.flat)) leak = true;
      const printsSecret = s.fetch || s.groupOut || (s.secret && PRINTERS.has(s.word));
      if (printsSecret && redirects) leak = true;
      flowing = printsSecret || (flowing && !sink);
    }
    out ||= flowing;
  }
  return { leak, out };
}

// The old coarse rule, kept for commands the scanner cannot parse.
const DUMP_ANYWHERE_RE = new RegExp(String.raw`\b(?:${[...DUMP_WORDS].join('|')})\b`);
const EXFIL_PIPE_RE = new RegExp(
  String.raw`\b${SOURCE_FRAGMENT}[^;\n]*?\|[^;\n]*?\b(?:${[...EGRESS_WORDS].join('|')})\b`
);

function coarse() {
  return (
    (SOURCE_ANYWHERE_RE.test(command) && (DUMP_ANYWHERE_RE.test(command) || FILE_REDIRECT_RE.test(command))) ||
    EXFIL_PIPE_RE.test(command)
  );
}

// Any scanner failure, not only an unbalanced quote, takes the coarse rule: a crash must never read as "allowed".
function blocks() {
  try {
    return analyze(command, new Set()).leak || (envLoaded && ENV_LISTING_RE.test(command));
  } catch {
    return coarse();
  }
}

process.exit(blocks() ? 0 : 1);
