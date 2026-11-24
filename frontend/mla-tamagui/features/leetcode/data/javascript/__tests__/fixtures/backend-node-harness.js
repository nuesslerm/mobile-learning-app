
let raw = '';
process.stdin.on('data', (c) => { raw += c; });

async function main() {
  const payload = JSON.parse(raw);
  const emit = (o) => process.stdout.write(JSON.stringify(o));
  if (payload.mode === 'format') {
    try {
      const prettier = require('prettier');
      const formatted = await prettier.format(payload.code, { parser: 'babel' });
      emit({ ok: true, formatted });
    } catch (e) {
      emit({ ok: false, formatted: '', error: String((e && e.message) || e) });
    }
    return;
  }
  const methodName = payload.methodName || 'twoSum';
  const norm = (x) => JSON.stringify(x);
  const compare = (a, e, cmp) => {
    if (cmp === 'sorted') return norm([...a].sort()) === norm([...e].sort());
    if (cmp === 'setOfSorted') return norm(a.map((x) => [...x].sort())) === norm(e.map((x) => [...x].sort()));
    return norm(a) === norm(e);
  };
  let Solution;
  try {
    // oxlint-disable-next-line no-eval -- running the submitted solution is this harness's job
    Solution = (0, eval)('(function(){' + payload.code + '; return typeof Solution!=="undefined"?Solution:null;})()');
  } catch (e) {
    emit({ ok: false, results: [], error: 'CompileError: ' + String((e && e.message) || e) });
    return;
  }
  if (!Solution) { emit({ ok: false, results: [], error: 'No Solution class defined' }); return; }
  const results = [];
  for (const tc of payload.testCases || []) {
    const inputs = tc.inputs || {};
    const args = Object.values(inputs);
    const started = Date.now();
    let actual = null, error = null, pass = false;
    try {
      const sol = new Solution();
      actual = sol[methodName](...args);
      pass = compare(actual, tc.expected, tc.compareMode || 'exact');
    } catch (e) {
      error = String((e && e.name ? e.name + ': ' : '') + ((e && e.message) || e));
    }
    results.push({ pass, input: inputs, actual, expected: tc.expected, stdout: '', stderr: '', error, elapsed_ms: Date.now() - started });
  }
  emit({ ok: true, results });
}

process.stdin.on('end', () => {
  main().catch((err) => {
    process.stderr.write(String((err && err.stack) || err) + '\n');
    process.exit(1);
  });
});
