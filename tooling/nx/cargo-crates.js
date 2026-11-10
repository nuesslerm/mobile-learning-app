// The repo's cargo workspaces as Nx projects, each task keyed by exactly the crate sources it builds from. A crate inside a pnpm
// package is that package's files unless it sets `"nx": {"cargoCrates": true}`; every other crate, and each workspace, is a project.
const { existsSync, readFileSync, readdirSync } = require('node:fs');
const { join, dirname, relative, resolve } = require('node:path');
const { parse } = require('smol-toml');

// Read from the manifests, not `cargo metadata`: the graph is built in every CI job, and only some of them have a toolchain
const toml = (root, file) => parse(readFileSync(join(root, file), 'utf8'));

function crates(root, manifest) {
  const ws = toml(root, manifest);
  const wsDir = dirname(manifest);
  // `members` is read literally, without globs: a new crate is no project until the workspace manifest lists it
  const dirs = [
    ...(ws.package ? [wsDir] : []),
    ...(ws.workspace?.members ?? []).map((m) => join(wsDir, m)),
  ];
  return dirs.map((dir) => {
    const m = toml(root, join(dir, 'Cargo.toml'));
    const sections = [
      m.dependencies,
      m['dev-dependencies'],
      m['build-dependencies'],
      ...Object.values(m.target ?? {}).flatMap((t) => [
        t.dependencies,
        t['dev-dependencies'],
      ]),
    ];
    const paths = sections
      .flatMap((deps) => Object.entries(deps ?? {}))
      .map(([key, spec]) => {
        const fromWorkspace =
          spec?.workspace === true ? ws.workspace?.dependencies?.[key] : null;
        if (fromWorkspace?.path) return join(wsDir, fromWorkspace.path);
        return spec?.path ? join(dir, spec.path) : null;
      });
    // a path dependency is named by its own manifest, whatever key the dependent gives it
    const deps = [
      ...new Set(
        paths
          .filter(Boolean)
          .map(
            (d) =>
              toml(root, join(relative(root, resolve(root, d)), 'Cargo.toml'))
                .package.name
          )
      ),
    ];
    const flow = m.package.metadata?.flow ?? {};
    const integration = m.package.metadata?.integration ?? {};
    return {
      name: m.package.name,
      dir: relative(root, resolve(root, dir)),
      manifest,
      deps,
      e2e: m.package.metadata?.e2e ?? null,
      // `[package.metadata.flow] platform`: true opts in to the platform tier, false opts out of the scan's verdict, absent leaves it to the scan
      platform: typeof flow.platform === 'boolean' ? flow.platform : null,
      // what the flow (and integration) tests read beyond the crate: `inputs` (files), `uses` (crates whose binaries they run); `groups`, `shared`
      flow: {
        inputs: flow.inputs ?? [],
        uses: flow.uses ?? [],
        shared: flow.shared ?? [],
        groups: flow.groups ?? {},
      },
      // what only the integration tests read: a chain that runs other crates' binaries and the files that name its rows
      integration: {
        inputs: integration.inputs ?? [],
        uses: integration.uses ?? [],
      },
    };
  });
}

// the pnpm package a directory sits in, if any
function packageOf(root, dir) {
  for (let d = dir; d && d !== '.'; d = dirname(d)) {
    if (existsSync(join(root, d, 'package.json'))) return d;
  }
  return null;
}

// What a crate adds to the flow and e2e tests that build it (`^native` in nx.json): its Rust toolchain. The platform is no part of it:
// a crate's tasks take the `platform` input only in the platform tier (below). No other project defines `native`.
const NATIVE = { native: ['nativeToolchain'] };

// Rust source with comments and string and char literals blanked, so a scan sees code only (a `cfg(unix)` in a test string is no cfg)
function rustCode(text) {
  const char = /'(?:\\[^']+|[^\\'])'/y;
  const raw = /b?r(#*)"/y;
  let out = '';
  let i = 0;
  while (i < text.length) {
    const c = text[i];
    raw.lastIndex = i;
    const rawOpen =
      (c === 'r' || c === 'b') && !/\w/.test(text[i - 1] ?? ' ')
        ? raw.exec(text)
        : null;
    char.lastIndex = i;
    if (c === '/' && text[i + 1] === '/') {
      while (i < text.length && text[i] !== '\n') i++;
    } else if (c === '/' && text[i + 1] === '*') {
      i += 2;
      for (let depth = 1; depth && i < text.length; ) {
        if (text.startsWith('/*', i)) {
          depth++;
          i += 2;
        } else if (text.startsWith('*/', i)) {
          depth--;
          i += 2;
        } else i++;
      }
      out += ' ';
    } else if (rawOpen) {
      const close = text.indexOf(`"${rawOpen[1]}`, raw.lastIndex);
      i = close < 0 ? text.length : close + 1 + rawOpen[1].length;
      out += ' ';
    } else if (c === '"') {
      for (
        i++;
        i < text.length && text[i] !== '"';
        i += text[i] === '\\' ? 2 : 1
      );
      i++;
      out += ' ';
    } else if (c === "'" && char.test(text)) {
      i = char.lastIndex;
      out += ' ';
    } else {
      out += c;
      i++;
    }
  }
  return out;
}

// The text inside the parens that open at `open`
const balanced = (code, open) => {
  let depth = 0;
  for (let i = open; i < code.length; i++) {
    if (code[i] === '(') depth++;
    else if (code[i] === ')' && --depth === 0) return code.slice(open + 1, i);
  }
  return code.slice(open + 1);
};

// A cfg that tells CPUs apart: `cfg!(..)`, `#[cfg(..)]`, `#![cfg(..)]` or `cfg_attr(..)` naming the architecture, pointer width or endianness.
// Not the OS or `unix`: every machine that writes the cache is Linux.
const CPU_CFG = /\btarget_(?:arch|pointer_width|endian)\b/;
const hasCpuCfg = (text) => {
  if (!CPU_CFG.test(text)) return false;
  const code = rustCode(text);
  return [...code.matchAll(/(?<![\w.])cfg(?:_attr)?\s*!?\s*\(/g)].some((m) =>
    CPU_CFG.test(balanced(code, m.index + m[0].length - 1))
  );
};

// The scan of one crate (its src, tests and benches and build.rs), once per graph build
const scanned = new Map();
const cpuDependent = (root, dir) => {
  const key = join(root, dir);
  if (!scanned.has(key)) {
    const files = ['src', 'tests', 'benches'].flatMap((d) =>
      existsSync(join(key, d))
        ? readdirSync(join(key, d), { recursive: true })
            .filter((f) => f.endsWith('.rs'))
            .map((f) => join(key, d, f))
        : []
    );
    if (existsSync(join(key, 'build.rs'))) files.push(join(key, 'build.rs'));
    scanned.set(
      key,
      files.some((f) => hasCpuCfg(readFileSync(f, 'utf8')))
    );
  }
  return scanned.get(key);
};

// A crate is in the platform tier when its manifest says so (`[package.metadata.flow] platform = true`) or, unless it says `false`, the scan finds CPU-dependent code.
// Only the crate's own flag counts: a dependent of a platform crate keeps the default tier.
const platformTier = (root, c) => c.platform ?? cpuDependent(root, c.dir);

// Inline unit tests: a `#[cfg(test)]` module anywhere under the crate's src
const hasUnitTests = (root, dir) => {
  const src = join(root, dir, 'src');
  return (
    existsSync(src) &&
    readdirSync(src, { recursive: true }).some(
      (f) => f.endsWith('.rs') && readFileSync(join(src, f), 'utf8').includes('#[cfg(test)]')
    )
  );
};

const packageJson = (root, dir) =>
  JSON.parse(readFileSync(join(root, dir, 'package.json'), 'utf8'));

// a Rust task runs no npm package; the npm graph differs by CPU, so naming none keeps the key the same in the sandbox and on the VM
const OWN = ['rust', '^rustProduction', { externalDependencies: [] }];

// What every build in a workspace reads besides the crates: its manifest and cargo config
const workspaceFiles = (manifest) => [manifest, join(dirname(manifest), '.cargo/config.toml')];

// The workspace's lockfile entries the crates reach, an input and not a dependency: another workspace building them has its own
const lockSlice = (manifest, names) => ({
  runtime: `node tooling/nx/cargo-lock-slice.js ${join(dirname(manifest), 'Cargo.lock')} ${[...new Set(names)].sort().join(' ')}`,
});

// the directory part of a glob, under which everything it matches lies
const globBase = (glob) => {
  const parts = glob.split('/');
  const wild = parts.findIndex((p) => /[*?[{]/.test(p));
  return (wild < 0 ? parts : parts.slice(0, wild)).join('/');
};
const under = (dir, parent) => parent === '' || dir.startsWith(`${parent}/`);

// A group entry with no `/`, `.` or `*` names a crate; any other is a file glob from the crate dir
const namesCrate = (entry) => !/[/.*]/.test(entry);

const unique = (inputs) => [...new Map(inputs.map((i) => [JSON.stringify(i), i])).values()];

const rustTask = (inputs, command) => ({
  executor: 'nx:run-commands',
  cache: true,
  inputs,
  outputs: [],
  options: { cwd: '{workspaceRoot}', command },
});

// package.json files are matched too, though only read: Nx re-runs a plugin when a file it matched changed, and a script's
// `cargo -p <crate>` is a dependency
exports.createNodes = [
  '**/{Cargo.toml,package.json}',
  (files, _options, context) => {
    const root = context.workspaceRoot;
    const workspaces = files.filter(
      (f) =>
        f.endsWith('Cargo.toml') &&
        !f.startsWith('references/') &&
        readFileSync(join(root, f), 'utf8').includes('[workspace]')
    );
    // path dependencies cross workspaces (service-kit builds clis/telemetry, the vnc listener clis/wf-kit and clis/wf-login),
    // so every crate is known first
    const all = new Map(
      workspaces.flatMap((m) => crates(root, m)).map((c) => [c.name, c])
    );
    const crateDirs = [...all.values()].map((c) => c.dir);
    const optedIn = (pkg) => packageJson(root, pkg).nx?.cargoCrates === true;
    // the package whose files a crate is, when it sits in one that did not opt in
    const heldBy = (c) => {
      const pkg = packageOf(root, c.dir);
      return pkg && !optedIn(pkg) ? pkg : null;
    };
    const projectOf = (c) =>
      heldBy(c) ? packageJson(root, heldBy(c)).name : `crate-${c.name}`;
    const named = (where) => (name) => {
      if (all.has(name)) return all.get(name);
      const reason = `${where} names "${name}", which is no crate of the repository's cargo workspaces (their \`members\`)`;
      // `run-many` shows a plugin's error only under --verbose, so the reason goes out first (`wf test run` runs plugins in process:
      // an isolated worker's stderr can be lost when Nx exits)
      process.stderr.write(`tooling/nx/cargo-crates.js: ${reason}\n`);
      throw new Error(reason);
    };
    // the crates given and every crate they reach through path dependencies
    const closure = (start) => {
      const seen = new Map();
      const visit = (c) => {
        if (seen.has(c.name)) return;
        seen.set(c.name, c);
        c.deps.filter((d) => all.has(d)).forEach((d) => visit(all.get(d)));
      };
      start.forEach(visit);
      return [...seen.values()];
    };
    // Repository-relative globs as file sets. A crate nested under a declared glob is a project of its own and is left out; Nx applies
    // a task's `!` sets to all its file sets, so a crate whose own directory a set names stays in. A repo-wide pattern (`**/x`) names
    // files of every directory, the crates' among them, so it leaves none out.
    const fileSets = (declared, fixed) => {
      const bases = [...declared, ...fixed].filter((g) => !g.startsWith('!')).map(globBase);
      const nested = crateDirs.filter(
        (d) =>
          declared.some((g) => !g.startsWith('!') && globBase(g) !== '' && under(d, globBase(g))) &&
          !bases.some((b) => b === d || under(b, d))
      );
      return unique(
        [...fixed, ...declared, ...nested.map((d) => `!${d}/**`)].map((g) => ({
          fileset: g.startsWith('!') ? `!{workspaceRoot}/${g.slice(1)}` : `{workspaceRoot}/${g}`,
        }))
      );
    };
    // What a task of `c` reads beyond the crate projects: its workspace's files and lockfile slice, and `declared`. Its `flow` tests add
    // `[package.metadata.flow] inputs` and `uses`, its `integration` tests `[package.metadata.flow] uses` and `[package.metadata.integration]`
    // `inputs` and `uses`; each used crate's sources, workspace files and lockfile slice are inputs of the task that runs its binary.
    const extra = (c, declared, tests) => {
      const usesOf = (key, list) => list.map(named(`${c.name}: [package.metadata.${key}] uses`));
      const used = tests
        ? [...usesOf('flow', c.flow.uses), ...(tests === 'integration' ? usesOf('integration', c.integration.uses) : [])]
        : [];
      const slices = new Map([[c.manifest, [c.name]]]);
      for (const u of used) slices.set(u.manifest, [...(slices.get(u.manifest) ?? []), u.name]);
      const sources = closure(used).flatMap((u) => [
        `${u.dir}/**`,
        ...['tests/**', 'benches/**', 'target/**', '**/*.md'].map((g) => `!${u.dir}/${g}`),
      ]);
      const read = tests === 'flow' ? c.flow.inputs : tests === 'integration' ? c.integration.inputs : [];
      const inputs = read.map((f) => join(c.dir, f));
      return [
        ...fileSets([...declared, ...inputs, ...sources], [...slices.keys()].flatMap(workspaceFiles)),
        ...[...slices].map(([m, names]) => lockSlice(m, names)),
      ];
    };
    const workspaceProject = (manifest) =>
      packageOf(root, dirname(manifest))
        ? null
        : `cargo-${dirname(manifest).replaceAll('/', '-')}`;
    const byManifest = new Map(workspaces.map((m) => [m, {}]));
    for (const m of workspaces) {
      if (workspaceProject(m))
        byManifest.get(m)[dirname(m)] = {
          name: workspaceProject(m),
          tags: ['rust'],
          namedInputs: NATIVE,
          targets: {},
        };
    }
    const packageDeps = {};
    const addTo = (pkg, deps) =>
      deps
        .filter(Boolean)
        .forEach((d) => (packageDeps[pkg] ??= new Set()).add(d));
    for (const c of all.values()) {
      const pkg = packageOf(root, c.dir);
      const deps = c.deps.filter((d) => all.has(d)).map((d) => all.get(d));
      if (heldBy(c)) {
        addTo(pkg, [workspaceProject(c.manifest), ...deps.filter((d) => heldBy(d) !== pkg).map(projectOf)]);
        continue;
      }
      // an opted-in package's crates are its dependencies, with their checks here and their tests in the package's scripts
      if (pkg) addTo(pkg, [`crate-${c.name}`]);
      const tier = platformTier(root, c) ? ['platform'] : [];
      const checks = [...OWN, ...tier, ...extra(c, [], false)];
      const unit = hasUnitTests(root, c.dir)
        ? { 'cargo-unit': rustTask(checks, `cargo test --locked -p ${c.name} ${existsSync(join(root, c.dir, 'src', 'lib.rs')) ? '--lib' : '--bins'} --manifest-path ${c.manifest}`) }
        : {};
      const clippy = {
        'cargo-clippy': rustTask(checks, `cargo clippy --locked -p ${c.name} --all-targets --manifest-path ${c.manifest} -- -D warnings`),
      };
      const own = (f) => join(c.dir, f);
      const testTarget = (file, command, inputs) =>
        !pkg && existsSync(join(root, c.dir, 'tests', file)) ? rustTask(inputs, command) : null;
      const flowCommand = `cargo test --locked -p ${c.name} --test flow --manifest-path ${c.manifest}`;
      const flow = testTarget('flow.rs', flowCommand, [...OWN, ...tier, ...extra(c, [], 'flow')]);
      // the built binaries as a black box with every dependency a fake, one test at a time on a port of its own, after the services are deployed
      const integration = testTarget('integration.rs', `cargo test --locked -p ${c.name} --test integration --manifest-path ${c.manifest} -- --test-threads=1`, [...OWN, ...tier, ...extra(c, [], 'integration')]);
      // a real Chromium or other real dependency the flow tests double: one test at a time, after the services are deployed
      const e2e = testTarget('e2e.rs', `cargo test --locked -p ${c.name} --test e2e --manifest-path ${c.manifest} -- --test-threads=1`, checks);
      // the same suite without the parity tests (a test whose path names `parity` compares a live model with another): minutes of model
      // time that only a change to the crate itself, or a full plan, is worth
      const e2eSmoke = testTarget('e2e.rs', `cargo test --locked -p ${c.name} --test e2e --manifest-path ${c.manifest} -- --test-threads=1 --skip parity`, checks);
      // the deployed service asked over its real interface: its result is a fact about the VM, so no cache replays it
      const verifyDeployed = testTarget('deployed_verification.rs', `cargo test --locked -p ${c.name} --test deployed_verification --manifest-path ${c.manifest} -- --ignored --test-threads=1`, checks);
      if (verifyDeployed) verifyDeployed.cache = false;
      // the Nx cache's contract, proved against the real Nx on a copy of the repository: minutes of work, so a task of its own that
      // `wf test plan` selects only when a file that can move the contract changed. What decides a task's inputs is an input of it.
      const contract = testTarget('nx_contract.rs', `cargo test --locked -p ${c.name} --test nx_contract --manifest-path ${c.manifest}`, [
        ...checks,
        { fileset: '{workspaceRoot}/nx.json' },
        { fileset: '{workspaceRoot}/.nxignore' },
        { fileset: '{workspaceRoot}/tooling/nx/**/*' },
        { fileset: '{workspaceRoot}/**/project.json' },
        { fileset: '{workspaceRoot}/**/package.json' },
      ]);
      // A crate whose e2e suite is slow lists its groups in `[package.metadata.e2e]` (`groups`: a module of `tests/e2e.rs` each,
      // `shared`: files every group reads, `src`: the crate sources the suite exercises, `extra`: other repo files, all from the
      // crate dir): one `cargo-e2e-<group>` task each, keyed by those files alone, so a change reruns only the groups it can move.
      const groups = e2e && c.e2e ? c.e2e.groups ?? [] : [];
      const groupTargets = Object.fromEntries(
        groups.map((g) => [
          `cargo-e2e-${g}`,
          {
            ...e2e,
            inputs: [
              'nativeToolchain',
              '^rustProduction',
              { externalDependencies: [] },
              ...tier,
              ...extra(c, ['Cargo.toml', 'tests/e2e.rs', `tests/e2e/${g}.rs`, ...(c.e2e.shared ?? []), ...(c.e2e.src ?? []), ...(c.e2e.extra ?? [])].map(own), false),
            ],
            options: { cwd: '{workspaceRoot}', command: `${e2e.options.command} ${g}::` },
          },
        ])
      );
      // `[package.metadata.flow] groups` (`{<group> = [...]}`, a module of `tests/flow.rs` each): one `cargo-flow-<group>` task keyed by
      // the crate's own sources, its module, `shared`, the group's file globs, the crates it names and the deps no other group names.
      const flowGroups = flow ? Object.keys(c.flow.groups) : [];
      const namedBy = (g) =>
        c.flow.groups[g].filter(namesCrate).map(named(`${c.name}: [package.metadata.flow] groups.${g}`));
      const flowTargets = Object.fromEntries(
        flowGroups.map((g) => {
          const others = new Set(flowGroups.filter((o) => o !== g).flatMap((o) => namedBy(o).map((d) => d.name)));
          const reached = closure([...deps.filter((d) => !others.has(d.name)), ...namedBy(g)]).filter((d) => d.name !== c.name);
          const files = ['Cargo.toml', 'tests/flow.rs', `tests/flow/${g}.rs`, `tests/flow/${g}/**`, ...c.flow.shared, ...c.flow.groups[g].filter((e) => !namesCrate(e))];
          return [
            `cargo-flow-${g}`,
            rustTask(
              [
                'rustProduction',
                ...tier,
                ...(reached.length ? [{ input: 'rustProduction', projects: [...new Set(reached.map(projectOf))] }] : []),
                { externalDependencies: [] },
                ...extra(c, files.map(own), 'flow'),
              ],
              `${flowCommand} ${g}::`
            ),
          ];
        })
      );
      const targets = {
        ...(flow && !flowGroups.length ? { 'cargo-flow': flow } : {}),
        ...flowTargets,
        ...(integration ? { 'cargo-integration': integration } : {}),
        ...(verifyDeployed ? { 'cargo-verify-deployed': verifyDeployed } : {}),
        ...(contract ? { 'cargo-contract': contract } : {}),
        ...(e2e && !groups.length ? { 'cargo-e2e': e2e } : {}),
        ...groupTargets,
        ...(e2eSmoke ? { 'cargo-e2e-smoke': e2eSmoke } : {}),
      };
      byManifest.get(c.manifest)[c.dir] = {
        name: `crate-${c.name}`,
        tags: ['rust'],
        namedInputs: NATIVE,
        implicitDependencies: [...new Set(deps.map(projectOf))],
        targets: { ...clippy, ...targets, ...unit },
      };
    }
    // a pnpm package whose scripts run `cargo ... -p <crate>` for a crate outside it builds that crate too
    for (const file of files.filter(
      (f) => f.endsWith('package.json') && !f.startsWith('references/')
    )) {
      const scripts = Object.values(
        JSON.parse(readFileSync(join(root, file), 'utf8')).scripts ?? {}
      ).filter((s) => s.includes('cargo '));
      const pkg = dirname(file);
      for (const name of scripts.flatMap((s) =>
        [...s.matchAll(/ -p ([\w-]+)/g)].map((m) => m[1])
      )) {
        const c = all.get(name);
        if (c && packageOf(root, c.dir) !== pkg)
          addTo(pkg, [projectOf(c), workspaceProject(c.manifest)]);
      }
    }
    // An opted-in package's test:flow and test:integration list `cargoFlow` / `cargoIntegration` in its package.json: targetDefaults beat
    // a plugin's target inputs, and a named input takes no `projects`, so the suite's crate dir and flow inputs are file sets here.
    const suite = (pkg, script, tests) => {
      const text = packageJson(root, pkg).scripts?.[script] ?? '';
      const run = [...text.matchAll(/ -p ([\w-]+)/g)]
        .map((m) => all.get(m[1]))
        .filter((c) => c && packageOf(root, c.dir) === pkg);
      if (!run.length) return ['default'];
      return unique([
        'nativeToolchain',
        ...(run.some((c) => platformTier(root, c)) ? ['platform'] : []),
        '{projectRoot}/package.json',
        ...run.flatMap((c) => extra(c, [`${c.dir}/**`, `!${c.dir}/target/**`, `!${c.dir}/**/*.md`], tests)),
      ]);
    };
    const results = workspaces.map((m) => [m, { projects: byManifest.get(m) }]);
    const packages = Object.fromEntries(
      Object.entries(packageDeps).map(([pkg, deps]) => [
        pkg,
        {
          implicitDependencies: [...deps],
          ...(optedIn(pkg)
            ? { namedInputs: { cargoFlow: suite(pkg, 'test:flow', 'flow'), cargoIntegration: suite(pkg, 'test:integration', 'integration') } }
            : {}),
        },
      ])
    );
    if (results.length) Object.assign(results[0][1].projects, packages);
    return results;
  },
];
