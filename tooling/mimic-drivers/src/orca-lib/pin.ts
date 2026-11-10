import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { repoRoot } from '../lock.ts'

export type OrcaPin = { version: string; commit: string }

export function orcaRoot(): string {
  return process.env.MIMIC_ORCA_ROOT || join(repoRoot, 'references', 'orca')
}

export function readPin(): OrcaPin {
  const file = process.env.MIMIC_ORCA_PIN || join(repoRoot, 'tooling', 'orca-conformance', 'orca-pin.json')
  const pin = JSON.parse(readFileSync(file, 'utf8'))
  if (typeof pin.version !== 'string' || typeof pin.commit !== 'string') throw new Error(`${file} has no version and commit`)
  return pin
}

/** Recording against code that is not the pin would stamp a cassette with a commit it was not recorded at. */
export function assertAtPin(): OrcaPin {
  const pin = readPin()
  // A git hook exports GIT_DIR, which would make the submodule read as the parent repo.
  const env = Object.fromEntries(Object.entries(process.env).filter(([k]) => !k.startsWith('GIT_')))
  const head = execFileSync('git', ['-C', orcaRoot(), 'rev-parse', 'HEAD'], { encoding: 'utf8', env }).trim()
  if (head !== pin.commit) throw new Error(`${orcaRoot()} is at ${head}, the pin names ${pin.commit}: git submodule update --init references/orca`)
  return pin
}
