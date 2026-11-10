// Given the pinned Orca tree and the rows of `orca-methods.json`, the zod schemas of a namespace's phone methods derive payloads the schemas accept.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { phoneMethods, registeredParams, spike, derive } from '../src/orca/schema.ts'

test('every phone row of the files namespace derives a payload its own schema accepts, rows read from the rows file and not named here', async () => {
  // Given the phone rows of `files` and the methods the pinned tree registers for them
  const rows = phoneMethods('files')
  const registered = registeredParams('files').map((r) => r.method)
  assert.ok(rows.length > 0 && rows.every((m) => registered.includes(m)), `rows the pinned tree does not register: ${rows.filter((m) => !registered.includes(m))}`)
  // When payloads are derived from the schemas
  const report = await spike('files')
  // Then each row has one and the schema that derived it accepts it, with and without the optional keys
  assert.deepEqual(report.rows.map((r) => r.method), rows)
  const bad = report.rows.filter((r) => !(r.derived && r.valid && r.full_valid))
  assert.deepEqual(bad, [], 'rows whose derived payload its schema rejects')
})

test('the schemas reject most invalid shapes: a payload without its selector or with a wrong-typed one never parses', async () => {
  // Given the derived payloads
  const report = await spike('files')
  // When each is probed with no payload, an empty object, and each key dropped or replaced by a number
  // Then the selector probes are all rejected, and what is accepted is a key with a default or a number a number key allows
  const accepted = report.rows.flatMap((r) => (r.probes?.accepted ?? []).map((p) => `${r.method}:${p}`))
  assert.ok(accepted.every((p) => !/:(no-payload|empty-object|drop:worktree|number:worktree)$/.test(p)), `selector probes accepted: ${accepted}`)
  assert.equal(report.counts.probes_total - report.counts.probes_rejected, accepted.length)
})

test('a construct the walk does not know is named, not guessed', () => {
  // Given a schema made of a construct the walk has no case for
  const schema = { _zod: { def: { type: 'bigint' } }, safeParse: () => ({ success: true }) }
  // When a value is derived, Then the error names the construct
  assert.throws(() => derive(schema as never, false), /zod construct "bigint"/)
})
