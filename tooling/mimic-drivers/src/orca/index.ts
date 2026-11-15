// How the sidecar gets an Orca host: the one the environment names (target.ts), or an orcad provisioned at the pin when `ORCA_ORCAD_CHECKOUT`
// names a checkout of it (orcad.ts, the VM).
import { machinePorts, orcadLauncher } from './orcad.ts'
import { TARGET_ENV, targetFromEnv, type Target } from './target.ts'

export type Launch = <T>(use: (target: Target) => Promise<T>) => Promise<T>

export async function launcher(env: NodeJS.ProcessEnv): Promise<Launch> {
  const target = await targetFromEnv(env)
  if (target) return (use) => use(target)
  if (env.ORCA_ORCAD_CHECKOUT) return orcadLauncher(env.ORCA_ORCAD_CHECKOUT, machinePorts)
  throw new Error(`no Orca host: set ${TARGET_ENV}, or ORCA_ORCAD_CHECKOUT`)
}
