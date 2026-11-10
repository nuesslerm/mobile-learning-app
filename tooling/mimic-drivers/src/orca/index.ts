// The Orca ws driver: `script` runs a request list on the phone's own pinned transport against the host the environment names
// (target.ts), or against an orcad provisioned at the pin when `ORCA_ORCAD_CHECKOUT` names a checkout of it (orcad.ts, the VM).
import type { Driver } from '../protocol.ts'
import { shared } from '../session.ts'
import { toolchainLockHash } from '../lock.ts'
import { readPin } from '../orca-lib/pin.ts'
import { runScript, type Script } from './client.ts'
import { machinePorts, orcadLauncher } from './orcad.ts'
import { TARGET_ENV, targetFromEnv, type Target } from './target.ts'

export type Launch = <T>(use: (target: Target) => Promise<T>) => Promise<T>

async function launcher(env: NodeJS.ProcessEnv): Promise<Launch> {
  const target = await targetFromEnv(env)
  if (target) return (use) => use(target)
  if (env.ORCA_ORCAD_CHECKOUT) return orcadLauncher(env.ORCA_ORCAD_CHECKOUT, machinePorts)
  throw new Error(`no Orca host: set ${TARGET_ENV}, or ORCA_ORCAD_CHECKOUT`)
}

/** One orcad for many interactions (its start costs seconds); `reset` starts a new one, `MIMIC_SESSION=fresh` starts one per interaction. */
export function orca(launch: (env: NodeJS.ProcessEnv) => Promise<Launch> = launcher, opts: { session?: boolean } = {}): Driver {
  const session = opts.session ?? (launch === launcher && process.env.MIMIC_SESSION !== 'fresh')
  const held = session ? shared<Target>((use) => launch(process.env).then((l) => l(use))) : undefined
  return {
    name: 'mimic-drivers/orca',
    version: '1',
    protocol: 'mimic-driver/1',
    toolchainLockHash: toolchainLockHash(),
    targetVersion: readPin().version,
    ops: { script: async (input: Script) => (held ? runScript((await held.get()).server, input) : (await launch(process.env))((target) => runScript(target, input))) },
    reset: async () => held?.reset(),
    close: async () => held?.close()
  }
}
