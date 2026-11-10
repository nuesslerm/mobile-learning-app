// `node tooling/nx/cargo-lock-slice.js <Cargo.lock> <crate>...`, an Nx runtime input: the hash of the lock entries the crates'
// closure reaches (as `wf ship` keys images), or of the whole file when it does not parse or lacks a crate.
const { createHash } = require('node:crypto');
const { existsSync, readFileSync } = require('node:fs');
const { parse } = require('smol-toml');

const sha = (text) => createHash('sha256').update(text).digest('hex');

function closure(text, roots) {
  let lock;
  try {
    lock = parse(text);
  } catch {
    return null;
  }
  const byName = new Map();
  for (const p of Array.isArray(lock.package) ? lock.package : []) {
    if (!byName.has(p.name)) byName.set(p.name, []);
    byName.get(p.name).push(p);
  }
  // a dependency is written `name`, `name version` or `name version (source)`
  const find = (spec) => {
    const [name, version, source] = String(spec).split(' ');
    return (byName.get(name) ?? []).find(
      (p) => (!version || p.version === version) && (!source || `(${p.source})` === source)
    );
  };
  const start = roots.map((r) => (byName.get(r) ?? []).find((p) => !p.source));
  if (start.some((p) => !p)) return null;
  const seen = new Map();
  for (const stack = [...start]; stack.length; ) {
    const p = stack.pop();
    const key = [p.name, p.version, p.source ?? '', p.checksum ?? ''].join(' ');
    if (seen.has(key)) continue;
    seen.set(key, p);
    stack.push(...(p.dependencies ?? []).map(find).filter(Boolean));
  }
  return [...seen.keys()].sort();
}

const [lockfile, ...roots] = process.argv.slice(2);
if (!lockfile || !existsSync(lockfile)) {
  process.stdout.write('no lockfile\n');
} else {
  const text = readFileSync(lockfile, 'utf8');
  const entries = closure(text, roots);
  process.stdout.write(entries ? `slice ${sha(entries.join('\n'))}\n` : `whole ${sha(text)}\n`);
}
