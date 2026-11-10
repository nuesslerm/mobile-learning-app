// The hash a cassette's provenance carries for the driver's toolchain: the dependencies of tooling/mimic-drivers and the
// `packages:` entries of what it resolves, as pnpm-lock.yaml (the workspace lockfile) has them. Changes only when the pins change.
import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
export const packageDir = join(here, '..')
export const repoRoot = join(packageDir, '..', '..')

function block(lines: string[], header: string, indent: number): string[] {
  const start = lines.indexOf(header)
  if (start < 0) throw new Error(`pnpm-lock.yaml has no ${header.trim()}`)
  const out = [header]
  for (const line of lines.slice(start + 1)) {
    if (line.trim() !== '' && line.length - line.trimStart().length <= indent) break
    out.push(line)
  }
  return out
}

function pinnedPackages(lines: string[], importer: string[], only?: string[], withEntries = false): string[] {
  const found = [...importer.join('\n').matchAll(/^ {6}'?([^':\s]+\/?[^':\s]*)'?:\n {8}specifier: .*\n {8}version: (\d\S*)$/gm)].filter((m) => !only || only.includes(m[1]!))
  const names = found.map((m) => `${m[1]}@${m[2]}`)
  return [...(withEntries ? found.map((m) => m[0]) : []), ...names.flatMap((n) => {
    const key = lines.findIndex((l) => l === `  '${n}':` || l === `  ${n}:`)
    if (key < 0) throw new Error(`pnpm-lock.yaml has no packages entry for ${n}`)
    return block(lines, lines[key]!, 2)
  })]
}

/** A workspace package the reference runs against: the named runtime dependencies of its own lock slice join the hash. */
export type LibraryPins = { importer: string; packages: string[] }

export function toolchainLockHash(lockText = readFileSync(join(repoRoot, 'pnpm-lock.yaml'), 'utf8'), library?: LibraryPins): string {
  const lines = lockText.split('\n')
  // the runtime dependencies only: a lint or typecheck tool moving must not make every cassette look stale
  const importer = block(block(lines, '  tooling/mimic-drivers:', 2), '    dependencies:', 4)
  const parts = [...importer, ...pinnedPackages(lines, importer)]
  if (library) {
    const deps = block(block(lines, `  ${library.importer}:`, 2), '    dependencies:', 4)
    parts.push(`importer ${library.importer}`, ...pinnedPackages(lines, deps, library.packages, true))
  }
  return 'sha256:' + createHash('sha256').update(parts.join('\n')).digest('hex')
}
