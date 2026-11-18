// node src/cli.ts serve <target>: run one driver as a mimic-driver/1 server on stdin and stdout.
// node src/cli.ts schema orca <namespace>: derive the phone rows' payloads of a namespace from the pinned zod schemas (the X6 spike);
// node src/cli.ts crawl <orca|t3> [--runtime <dir>]: the endpoint inventory and request corpus of the pinned source (mimic-crawl-report/1); `wf mimic crawl` runs it.
// node src/cli.ts refresh-corpus <orca|t3> <set dir...>: the corpus of each shard of an app after a pin move, the cases it holds kept and the new rows dealt to the smallest shard.
// node src/cli.ts scenarios <orca|t3|t3-agent|t3-methods> [<set dir>]: derive the stateful scenarios of an app from its committed crawl (corpus.json and scenarios.json of the scenario set).
// node src/cli.ts capture-runtime t3 <out dir>: the release binary's --help of every subcommand and the descriptor of a server it starts, as `crawl t3 --runtime <out dir>` reads them.
// node src/cli.ts sidecar orca: the phone's pinned client behind JSON lines on stdio, the transport of the Rust step interpreter for Orca.
// node src/cli.ts corpus <orca|t3> <set dir>: write corpus.json and coverage.json (every enumerated row and what became of it) of a recording set.
// with --cases the payloads as corpus cases of the `script` op.
import { serve } from './protocol.ts'
import { serveSidecar } from './orca/sidecar.ts'
import { orcaLib } from './orca-lib/index.ts'
import { cases, spike } from './orca/schema.ts'
import { refreshCorpus, writeCorpus } from './corpus/index.ts'
import { crawl } from './crawl/index.ts'
import { writeAgentCues, writeAgentMethods, writeAgentQueries, writeAgentReal, writeAgentTurns, writeScenarios } from './scenarios/index.ts'
import { voiceEditDsl } from './voice-edit-dsl/index.ts'

const TARGETS = { 'orca-lib': orcaLib, 'voice-edit-dsl': voiceEditDsl }

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
if (verb === 'refresh-corpus' && (name === 'orca' || name === 't3') && namespace) {
  process.stdout.write(`${JSON.stringify(await refreshCorpus(name, process.argv.slice(4)))}\n`)
  process.exit(0)
}
if (verb === 'scenarios' && name === 't3-agent') {
  process.stdout.write(`${JSON.stringify(writeAgentTurns(namespace))}\n`)
  process.exit(0)
}
if (verb === 'scenarios' && name === 't3-methods') {
  process.stdout.write(`${JSON.stringify(writeAgentMethods(namespace))}\n`)
  process.exit(0)
}
if (verb === 'scenarios' && name === 't3-cues') {
  process.stdout.write(`${JSON.stringify(writeAgentCues(namespace))}\n`)
  process.exit(0)
}
if (verb === 'scenarios' && name === 't3-real') {
  process.stdout.write(`${JSON.stringify(writeAgentReal(namespace))}\n`)
  process.exit(0)
}
if (verb === 'scenarios' && name === 't3-queries') {
  process.stdout.write(`${JSON.stringify(writeAgentQueries(namespace))}\n`)
  process.exit(0)
}
if (verb === 'scenarios' && (name === 'orca' || name === 't3')) {
  process.stdout.write(`${JSON.stringify(writeScenarios(name, namespace))}\n`)
  process.exit(0)
}
if (verb === 'capture-runtime' && name === 't3' && namespace) {
  const { captureRuntime } = await import('./crawl/runtime.ts')
  process.stdout.write(`${JSON.stringify(await captureRuntime(namespace))}\n`)
  process.exit(0)
}
if (verb === 'sidecar' && name === 'orca') {
  await serveSidecar(process.stdin, (line) => void process.stdout.write(`${line}\n`))
  process.exit(0)
}
const make = TARGETS[name as keyof typeof TARGETS]
if (verb !== 'serve' || !make) {
  process.stderr.write(`usage: node src/cli.ts serve <${Object.keys(TARGETS).join('|')}>\n`)
  process.exit(2)
}
await serve(make(), process.stdin, process.stdout)
