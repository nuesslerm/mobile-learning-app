import { execFileSync, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, realpathSync, unlinkSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const DENY_FILE = join(dirname(fileURLToPath(import.meta.url)), 'deny.json');
export const APPROVE_FILE = '.agent-guard-approve';

const RULE_KEYS = new Set(['command', 'args_sequential', 'args_any_of', 'file_path', 'condition', 'ask', 'reason']);

/** Returns `{ rules }` or `{ error }`; an unusable list must fail closed for Bash. */
export function loadRules(path = DENY_FILE) {
  let rules;
  try {
    rules = JSON.parse(readFileSync(path, 'utf8')).rules;
  } catch (error) {
    return { error: `${path}: ${error instanceof Error ? error.message : String(error)}` };
  }
  if (!Array.isArray(rules)) return { error: `${path}: "rules" must be an array` };
  const problems = rules.flatMap((rule, i) => ruleProblems(rule).map((p) => `rules[${i}]: ${p}`));
  return problems.length > 0 ? { error: `${path}: ${problems.join('; ')}` } : { rules };
}

function ruleProblems(rule) {
  if (rule === null || typeof rule !== 'object') return ['not an object'];
  const problems = Object.keys(rule).filter((k) => !RULE_KEYS.has(k)).map((k) => `unknown key "${k}"`);
  if (typeof rule.reason !== 'string' || rule.reason.length === 0) problems.push('"reason" must be a non-empty string');
  if ((typeof rule.command === 'string') === (typeof rule.file_path === 'string')) {
    problems.push('exactly one of "command" or "file_path" is required');
  }
  const patterns = [rule.command, rule.file_path, ...(rule.args_sequential ?? []), ...(rule.args_any_of ?? [])];
  for (const p of patterns.filter((x) => x !== undefined)) {
    try {
      new RegExp(p);
    } catch {
      problems.push(`invalid regex ${JSON.stringify(p)}`);
    }
  }
  return problems;
}

/** Splits a shell command into the simple commands it would run, including substitutions and heredoc-free bodies. */
export function splitCommands(command) {
  const segments = [];
  parse(command, 0, null, segments);
  return segments;
}

function parse(src, start, closer, segments) {
  let tokens = [];
  let word = null;
  let dropNextWord = false;
  let depth = 0;
  const pendingHeredocs = [];
  const flushWord = () => {
    if (word !== null && !dropNextWord) tokens.push(word);
    if (word !== null) dropNextWord = false;
    word = null;
  };
  const flushSegment = () => {
    flushWord();
    if (tokens.length > 0) segments.push(tokens);
    tokens = [];
  };
  const substitution = (from, close) => {
    const end = parse(src, from, close, segments);
    word = (word ?? '') + '$()';
    return end;
  };

  let i = start;
  while (i < src.length) {
    const c = src[i];
    const two = src.slice(i, i + 2);
    if (closer && c === closer && (closer === '`' || depth === 0)) {
      flushSegment();
      return i;
    }
    if (c === '\\') {
      if (src[i + 1] !== '\n') word = (word ?? '') + (src[i + 1] ?? '');
      i += 2;
    } else if (c === "'") {
      const end = src.indexOf("'", i + 1);
      if (end === -1) return finishUnclosed(src, segments);
      word = (word ?? '') + src.slice(i + 1, end);
      i = end + 1;
    } else if (c === '"') {
      word = word ?? '';
      i++;
      while (i < src.length && src[i] !== '"') {
        if (src[i] === '\\' && '"\\$`'.includes(src[i + 1] ?? '')) {
          word += src[i + 1];
          i += 2;
        } else if (src.startsWith('$(', i)) {
          i = substitution(i + 2, ')') + 1;
        } else if (src[i] === '`') {
          i = substitution(i + 1, '`') + 1;
        } else {
          word += src[i++];
        }
      }
      if (i >= src.length) return finishUnclosed(src, segments);
      i++;
    } else if (two === '$(' || two === '<(' || two === '>(') {
      if (two !== '$(') flushWord();
      i = substitution(i + 2, ')') + 1;
      if (two !== '$(') word = null;
    } else if (c === '`') {
      i = substitution(i + 1, '`') + 1;
    } else if (c === '#' && word === null) {
      while (i < src.length && src[i] !== '\n') i++;
    } else if (c === '\n') {
      flushSegment();
      i = skipHeredocBodies(src, i + 1, pendingHeredocs.splice(0));
    } else if (two === '&&' || two === '||' || two === '|&' || two === ';;') {
      flushSegment();
      i += 2;
    } else if (two === '&>' || c === '>' || c === '<') {
      if (word !== null && /^\d+$/.test(word)) word = null;
      flushWord();
      if (src.startsWith('<<<', i)) i += 3;
      else if (two === '<<') {
        i += src[i + 2] === '-' ? 3 : 2;
        const delim = /^\s*(['"]?)([^\s'";|&<>()]+)\1/.exec(src.slice(i));
        if (delim) {
          pendingHeredocs.push(delim[2]);
          i += delim[0].length;
        }
        continue;
      } else i += two === '&>' || two === '>>' || two === '>&' || two === '<&' || two === '>|' ? 2 : 1;
      if (src[i] === '&' || src[i - 1] === '&') {
        while (/[\d-]/.test(src[i] ?? '')) i++;
        continue;
      }
      dropNextWord = true;
    } else if (c === '|' || c === '&' || c === ';') {
      flushSegment();
      i++;
    } else if (c === '(' || c === ')') {
      if (c === '(') depth++;
      else depth--;
      flushSegment();
      i++;
    } else if (c === ' ' || c === '\t') {
      flushWord();
      i++;
    } else {
      word = (word ?? '') + c;
      i++;
    }
  }
  flushSegment();
  return i;
}

function skipHeredocBodies(src, from, delimiters) {
  let i = from;
  for (const delim of delimiters) {
    while (i < src.length) {
      const eol = src.indexOf('\n', i);
      const line = src.slice(i, eol === -1 ? src.length : eol);
      i = eol === -1 ? src.length : eol + 1;
      if (line.trim() === delim) break;
    }
  }
  return i;
}

// Unbalanced quotes: the shell would refuse it too, so match on the raw text as one opaque word.
function finishUnclosed(src, segments) {
  segments.length = 0;
  segments.push([src]);
  return src.length;
}

const SUDO_VALUE_OPTS = new Set(['-u', '-g', '-C', '-D', '-h', '-p', '-r', '-t', '-T', '-U']);
const ENV_VALUE_OPTS = new Set(['-u', '--unset', '-C', '--chdir', '-S', '--split-string']);
const ASSIGNMENT = /^[A-Za-z_][A-Za-z0-9_]*=/;

/** Drops what only launches the real command: `VAR=x`, sudo, env, command, rtk, `{`, `!`. */
export function stripWrappers(tokens) {
  let t = tokens;
  for (let guard = 0; guard < 20 && t.length > 0; guard++) {
    const head = t[0];
    if (ASSIGNMENT.test(head) || head === '{' || head === '!' || head === 'rtk') t = t.slice(1);
    else if (head === 'sudo') t = skipOptions(t.slice(1), SUDO_VALUE_OPTS);
    else if (head === 'env') t = skipOptions(t.slice(1), ENV_VALUE_OPTS);
    else if (head === 'command') t = skipOptions(t.slice(1), new Set());
    else break;
  }
  return t;
}

function skipOptions(tokens, valueOpts) {
  let i = 0;
  while (i < tokens.length && tokens[i].startsWith('-')) {
    if (tokens[i] === '--') return tokens.slice(i + 1);
    i += valueOpts.has(tokens[i]) ? 2 : 1;
  }
  return tokens.slice(i);
}

const SHELLS = new Set(['bash', 'sh', 'zsh', 'dash']);

/** Every simple command the string runs, wrappers stripped, `bash -c`/`eval` bodies expanded. */
function commandsIn(command, depth = 0) {
  return splitCommands(command).flatMap((raw) => {
    const tokens = stripWrappers(raw);
    if (tokens.length === 0) return [];
    const self = [{ raw, tokens }];
    if (depth > 4) return self;
    const name = basename(tokens[0]);
    const c = tokens.findIndex((t, i) => i > 0 && /^-[a-z]*c[a-z]*$/.test(t));
    if (SHELLS.has(name) && c !== -1 && tokens[c + 1] !== undefined) {
      return [...self, ...commandsIn(tokens[c + 1], depth + 1)];
    }
    if (name === 'eval') return [...self, ...commandsIn(tokens.slice(1).join(' '), depth + 1)];
    return self;
  });
}

const basename = (token) => token.split('/').pop() ?? token;
const normalizeArg = (arg) => (arg.startsWith('./') ? arg.slice(2) : arg);
const compile = (pattern) => new RegExp(normalizeArg(pattern));

function argsMatch(rule, args) {
  const norm = args.map(normalizeArg);
  if (rule.args_any_of && !rule.args_any_of.some((p) => norm.some((a) => compile(p).test(a)))) return false;
  let at = 0;
  for (const p of rule.args_sequential ?? []) {
    const re = compile(p);
    while (at < norm.length && !re.test(norm[at])) at++;
    if (at++ >= norm.length) return false;
  }
  return true;
}

function commandRuleMatches(rule, tokens) {
  if (typeof rule.command !== 'string') return false;
  return new RegExp(`^(?:${rule.command})$`).test(basename(tokens[0])) && argsMatch(rule, tokens.slice(1));
}

/** Runs a rule's `condition` script; exit 0 means the rule applies. */
export function runCondition(script, command, tokens, root) {
  const env = { ...process.env, GUARD_COMMAND: command };
  tokens.forEach((t, i) => (env[`GUARD_ARG_${i}`] = t));
  return spawnSync(process.execPath, [resolve(root, script)], { env, stdio: 'ignore' }).status === 0;
}

/** First rule hit across the command's simple commands, honouring each rule's condition. */
export function matchCommand(command, rules, { root, condition = runCondition } = {}) {
  for (const { raw, tokens } of commandsIn(command)) {
    const rule = rules.find(
      (r) => commandRuleMatches(r, tokens) && (!r.condition || condition(r.condition, command, tokens, root))
    );
    if (rule) return { rule, segment: raw.join(' ') };
  }
  return null;
}

function matchFile(filePath, rules, root) {
  const rel = repoRelative(filePath, root);
  return rules.find((r) => typeof r.file_path === 'string' && new RegExp(r.file_path).test(rel)) ?? null;
}

function repoRelative(filePath, root) {
  if (!isAbsolute(filePath)) return filePath;
  const canonical = realpathOfNearest(filePath);
  const inRoot = relative(realpathOfNearest(root), canonical);
  if (!inRoot.startsWith('..') && !isAbsolute(inRoot)) return inRoot;
  try {
    const top = execFileSync('git', ['-C', nearestDir(filePath), 'rev-parse', '--show-toplevel'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim();
    return relative(realpathOfNearest(top), canonical);
  } catch {
    return filePath;
  }
}

function nearestDir(p) {
  let dir = dirname(p);
  while (!existsSync(dir) && dir !== dirname(dir)) dir = dirname(dir);
  return dir;
}

function realpathOfNearest(p) {
  let tail = '';
  let at = p;
  while (!existsSync(at) && at !== dirname(at)) {
    tail = join(at.slice(dirname(at).length + 1), tail);
    at = dirname(at);
  }
  try {
    return join(realpathSync(at), tail);
  } catch {
    return p;
  }
}

export const commandToken = (segment) => createHash('sha256').update(segment).digest('hex').slice(0, 16);

function approvalMessage(reason, command, segment, approveFile) {
  return [
    `APPROVAL REQUIRED — ${reason}`,
    `Command: ${command}`,
    '',
    'STOP. Ask the user (question tool: "Allow this command to run?", Yes/No) and wait.',
    'If YES, make two separate Bash calls, no && or pipes:',
    `  1. printf '${commandToken(segment)}' > ${approveFile}`,
    `  2. ${command}`,
    'If NO, do nothing. Each approval is one-time only.',
  ].join('\n');
}

/**
 * The PreToolUse decision for one payload: a deny reason, or null to let it through.
 * Edit/Write never fail closed: they are how a broken deny list gets fixed.
 */
export function decide(payload, { root, loaded = loadRules(), condition } = {}) {
  const tool = payload?.tool_name;
  const input = payload?.tool_input ?? {};
  if (tool === 'Edit' || tool === 'Write') {
    if (loaded.error || typeof input.file_path !== 'string') return null;
    const rule = matchFile(input.file_path, loaded.rules, root);
    return rule ? `BLOCKED by approve guard: ${rule.reason}\nFile: ${input.file_path}` : null;
  }
  if (tool !== 'Bash' || typeof input.command !== 'string') return null;
  if (loaded.error) {
    return `BLOCKED by approve guard: its deny list is invalid, so every Bash call is refused.\n${loaded.error}\nFix it with Edit, then retry.`;
  }
  const hit = matchCommand(input.command, loaded.rules, { root, condition });
  if (!hit) return null;
  if (!hit.rule.ask) return `BLOCKED by approve guard: ${hit.rule.reason}\nCommand: ${input.command}`;
  const approveFile = join(root, APPROVE_FILE);
  try {
    if (readFileSync(approveFile, 'utf8').trim() === commandToken(hit.segment)) {
      unlinkSync(approveFile);
      return null;
    }
  } catch {
    // no approval on file
  }
  return approvalMessage(hit.rule.reason, input.command, hit.segment, approveFile);
}

export const denyOutput = (reason) =>
  JSON.stringify({
    hookSpecificOutput: { hookEventName: 'PreToolUse', permissionDecision: 'deny', permissionDecisionReason: reason },
  });
