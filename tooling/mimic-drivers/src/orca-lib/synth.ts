// The synthetic bundle files of the mobile-bundle set, generated from the case input (a generator name and its parameters) so the
// repository holds no 195 KB filler: the same input always gives the same bytes.
import { createHash } from 'node:crypto'

export type FileGen =
  | { kind: 'literal'; text: string }
  | { kind: 'text'; bytes: number; seed: number }
  | { kind: 'noise'; bytes: number }
  | { kind: 'empty' }

export type FileSpec = { path: string; type: string; gen: FileGen }

const sha = (b: Uint8Array) => createHash('sha256').update(b).digest('hex')

export function sha256Hex(bytes: Uint8Array): string {
  return sha(bytes)
}

function noise(n: number): Buffer {
  const bytes: number[] = []
  let h: Buffer = createHash('sha256').update('noise').digest()
  while (bytes.length < n) {
    bytes.push(...h)
    h = createHash('sha256').update(h).digest()
  }
  return Buffer.from(bytes.slice(0, n))
}

export function generate(gen: FileGen): Buffer {
  switch (gen.kind) {
    case 'literal':
      return Buffer.from(gen.text)
    case 'text':
      return Buffer.from(Array.from({ length: gen.bytes }, (_, i) => `const v${i % 97}=${(i * gen.seed) % 1009};\n`).join('')).subarray(0, gen.bytes)
    case 'noise':
      return noise(gen.bytes)
    case 'empty':
      return Buffer.alloc(0)
  }
}

export function materialize(files: FileSpec[]): Map<string, { type: string; data: Buffer }> {
  return new Map(files.map((f) => [f.path, { type: f.type, data: generate(f.gen) }]))
}
