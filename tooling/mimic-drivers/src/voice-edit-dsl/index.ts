// Ops over the TypeScript voice-edit DSL (frontend/libraries/voice-edit-dsl), the reference of the Rust port in
// backend/remote-chat-agent/rs. The library is bundled from its source at the tree the cassette's pin names.
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { bundleModule } from '../bundle.ts'
import { toolchainLockHash, type LibraryPins } from '../lock.ts'
import { ret, type Driver, type Json, type Op } from '../protocol.ts'

type Dsl = {
  completeBlockBodies(alternative: unknown, code: string, language: string): Json
  editSizeLimitsFor(transcript: string): Json
  normalizeVoiceEditResponse(input: unknown): Json
  repairJsonControlChars(text: string): string
  validateCommands(code: string, commands: unknown, limits: unknown): Json
}

const WHOLE_SOLUTION = 'write a two pointer solution'
const root = join(dirname(fileURLToPath(import.meta.url)), '../../../..')
const library = join(root, 'frontend/libraries/voice-edit-dsl')
// the app runs the library against its own zod, so the reference does too and that pin joins the toolchain hash
const pins: LibraryPins = { importer: 'frontend/libraries/voice-edit-dsl', packages: ['zod'] }

let dsl: Promise<Dsl> | undefined
const load = (): Promise<Dsl> => (dsl ??= bundleModule<Dsl>(`export * from '${library}/src/index'\n`, library))

// JSON.stringify drops what the library leaves undefined, the same as the file the cassette replaces did.
const plain = (v: unknown): Json => JSON.parse(JSON.stringify(v ?? null)) as Json

const ops: Record<string, Op> = {
  'dsl.validate': async (i: { code: string; commands: unknown; whole: boolean }) => {
    const d = await load()
    const limits = i.whole ? d.editSizeLimitsFor(WHOLE_SOLUTION) : d.editSizeLimitsFor('add a loop')
    return ret(plain(d.validateCommands(i.code, i.commands, limits)))
  },
  'dsl.blocks': async (i: { code: string; language: string; alternative: unknown }) => ret(plain((await load()).completeBlockBodies(i.alternative, i.code, i.language))),
  'dsl.repair': async (i: { text: string }) => ret((await load()).repairJsonControlChars(i.text)),
  'dsl.limits': async (i: { transcript: string }) => ret(plain((await load()).editSizeLimitsFor(i.transcript))),
  'dsl.normalize': async (i: { input: unknown }) => ret(plain((await load()).normalizeVoiceEditResponse(i.input)))
}

export function voiceEditDsl(): Driver {
  return { name: 'mimic-drivers/voice-edit-dsl', version: '1', protocol: 'mimic-driver/1', toolchainLockHash: toolchainLockHash(undefined, pins), ops }
}
