// node src/cli.ts serve <target>: run one driver as a mimic-driver/1 server on stdin and stdout.
// node src/cli.ts schema orca <namespace>: derive the phone rows' payloads of a namespace from the pinned zod schemas (the X6 spike);
// node src/cli.ts crawl <orca|t3> [--runtime <dir>]: the endpoint inventory and request corpus of the pinned source (mimic-crawl-report/1); `wf mimic crawl` runs it.
// node src/cli.ts scenarios <orca|t3> [<set dir>]: derive the stateful scenarios of an app from its committed crawl (corpus.json and scenarios.json of the scenario set).
// node src/cli.ts corpus <orca|t3> <set dir>: write corpus.json and coverage.json (every enumerated row and what became of it) of a recording set.
// with --cases the payloads as corpus cases of the `script` op.
import { serve } from './protocol.ts'
import { orca } from './orca/index.ts'
import { orcaLib } from './orca-lib/index.ts'
import { cases, spike } from './orca/schema.ts'
import { t3 } from './t3/index.ts'
import { writeCorpus } from './corpus/index.ts'
import { crawl } from './crawl/index.ts'
import { writeScenarios } from './scenarios/index.ts'
import { voiceEditDsl } from './voice-edit-dsl/index.ts'

const TARGETS = { orca, 'orca-lib': orcaLib, 'voice-edit-dsl': voiceEditDsl, t3 }

const [verb, name, namespace, flag] = process.argv.slice(2)
if (verb === 'crawl' && (name === 'orca' || name === 't3')) {
  const report = await crawl(name, process.argv.slice(4))
  process.stdout.write(`${JSON.stringify(report)}\n`, () => process.exit(0))
  await new Promise(() => {})
}
if (verb === 'schema' && name === 'orca' && namespace) {
  const report = await spike(namespace)
  process.stdout.write(`${JSON.stringify(flag === '--cases' ? cases(report) : report)}\n`)
  process.exit(0)
}
if (verb === 'corpus' && (name === 'orca' || name === 't3') && namespace) {
  process.stdout.write(`${JSON.stringify(await writeCorpus(name, namespace))}\n`)
  process.exit(0)
}
if (verb === 'scenarios' && (name === 'orca' || name === 't3')) {
  process.stdout.write(`${JSON.stringify(writeScenarios(name, namespace))}\n`)
  process.exit(0)
}
const make = TARGETS[name as keyof typeof TARGETS]
if (verb !== 'serve' || !make) {
  process.stderr.write(`usage: node src/cli.ts serve <${Object.keys(TARGETS).join('|')}>\n`)
  process.exit(2)
}
await serve(make(), process.stdin, process.stdout)
