// The phone's own transport: the pinned mobile `DirectRpcClient` (pairing key, v2 e2ee handshake, request tracker) bundled for Node.
// Two imports are stubbed because they serve the app shell, not the wire: AsyncStorage (the host latch is kept in memory, one fresh
// phone per run) and the translation helper (its fallback text only reaches human-readable log lines).
import { existsSync } from 'node:fs'
import { bundleModule, type Stub } from '../bundle.ts'
import { assertAtPin, orcaRoot } from '../orca-lib/pin.ts'

export type RpcResponse = { id: string; ok: true; result: unknown; _meta?: unknown } | { id: string; ok: false; error: { code: string; message: string } }

export type PhoneClient = {
  sendRequest(method: string, params?: unknown, options?: { timeoutMs?: number }): Promise<RpcResponse>
  /** A stream: every frame the host sends for it goes to `onData` (and a binary one to `options.onBinaryFrame`); the returned function ends it. */
  subscribe(method: string, params: unknown, onData: (result: unknown) => void, options?: { onBinaryFrame?: (frame: unknown) => void }): () => void
  getState(): string
  onStateChange(listener: (state: string) => void): () => void
  close(): void
}

export type PhoneModule = {
  DirectRpcClient: new (endpoint: string, deviceToken: string, serverPublicKeyB64: string, options: { onStateChange?: (s: string) => void; onLog?: (e: unknown) => void }) => PhoneClient
}

// fflate's gunzipSync is zlib's; expo-crypto's getRandomBytes is node's randomBytes.
const STUBS: Stub[] = [
  { filter: /^@react-native-async-storage\/async-storage$/, contents: `const m = new Map(); module.exports = { getItem: async (k) => m.get(k) ?? null, setItem: async (k, v) => void m.set(k, v), removeItem: async (k) => void m.delete(k) }` },
  { filter: /^fflate$/, contents: `const z = require('node:zlib'); module.exports = { gunzipSync: (b) => new Uint8Array(z.gunzipSync(b)) }` },
  { filter: /^expo-crypto$/, contents: `const c = require('node:crypto'); module.exports = { getRandomBytes: (n) => new Uint8Array(c.randomBytes(n)) }` },
  { filter: /\/i18n\/mobile-i18n$/, contents: `module.exports = { translate: (_key, fallback) => fallback }` }
]


/** Reports the e2ee v2 handshake the real client session performs, to whoever sets `globalThis.__mimicHandshakeTap` before the client is made:
 * the hello it sends, the ready it is given, the auth frame it seals first and the answer it opens first. The class itself is the pinned one. */
const handshakeTap = (real: string) =>
  `const real = require(${JSON.stringify(real)})
const Orig = real.PairedE2EEV2ClientSession
const create = Orig.create.bind(Orig)
// the client imports this module by two paths, so this stub is loaded twice over the one class: wrap it once
if (!Orig.__mimicTapped) {
Orig.__mimicTapped = true
const parse = (text) => { try { return JSON.parse(text) } catch { return text } }
Orig.create = (args) => {
  const session = create(args)
  const tap = globalThis.__mimicHandshakeTap
  if (!tap) return session
  tap({ stage: 'hello', dir: 'c2s', payload: session.hello })
  const accept = session.acceptReady.bind(session)
  session.acceptReady = (ready) => {
    tap({ stage: 'ready', dir: 's2c', payload: ready })
    const ok = accept(ready)
    if (!ok) tap({ stage: 'error', dir: 's2c', payload: { reason: 'ready not accepted' } })
    return ok
  }
  let sealed = 0
  const seal = session.sealText.bind(session)
  session.sealText = (plain) => {
    if (sealed++ === 0) tap({ stage: 'auth', dir: 'c2s', payload: parse(plain) })
    return seal(plain)
  }
  let opened = 0
  const open = session.openText.bind(session)
  session.openText = (frame) => {
    const out = open(frame)
    if (out !== null && opened++ === 0) tap({ stage: 'authenticated', dir: 's2c', payload: parse(out) })
    return out
  }
  return session
}
}
module.exports = real
`

/** The first of the pinned checkout's module paths (without the extension) that exists: Orca moves its sources between releases. */
function firstExisting(candidates: string[]): string {
  const found = candidates.find((c) => ['ts', 'tsx'].some((ext) => existsSync(`${orcaRoot()}/${c}.${ext}`)))
  if (!found) throw new Error(`none of ${candidates.join(', ')} exists in the Orca checkout at the pin: Orca moved the module again`)
  return found
}

let phone: Promise<PhoneModule> | undefined
let pairing: Promise<{ parsePairingCode(input: string): PairingOffer | null }> | undefined

export type PairingOffer = { v: number; endpoint: string; deviceToken: string; publicKeyB64: string; scope?: string }

/** The pinned parser of `orca://pair?code=...`: the offer an owner pairs a phone with, or an orcad prints when it is ready. */
export function loadPairing() {
  pairing ??= (async () => {
    assertAtPin()
    // Orca moved the parser from src/shared/pairing to the mobile transport (1.4.253) and then to mobile/src/pairing (1.4.258): every pin must load.
    const module = firstExisting(['src/shared/pairing', 'mobile/src/transport/pairing', 'mobile/src/pairing/pairing'])
    return bundleModule<{ parsePairingCode(input: string): PairingOffer | null }>(`export { parsePairingCode } from '${orcaRoot()}/${module}'\n`)
  })()
  return pairing
}

export function loadPhone(): Promise<PhoneModule> {
  phone ??= (async () => {
    assertAtPin()
    // The client logs progress with console.log; stdout carries the driver protocol.
    for (const level of ['log', 'info', 'warn', 'debug'] as const) console[level] = (...args: unknown[]) => void process.stderr.write(`${args.map(String).join(' ')}\n`)
    // The client moved into mobile/src/transport/rpc (1.4.258).
    const client = firstExisting(['mobile/src/transport/direct-rpc-client', 'mobile/src/transport/rpc/direct-rpc-client'])
    const session = `${orcaRoot()}/src/shared/remote-runtime/e2ee/paired-e2ee-v2-client-session.ts`
    const stubs: Stub[] = [...STUBS, { filter: /\/paired-e2ee-v2-client-session$/, contents: handshakeTap(session), resolveDir: orcaRoot() }]
    return bundleModule<PhoneModule>(`export { DirectRpcClient } from '${orcaRoot()}/${client}'\n`, undefined, stubs)
  })()
  return phone
}
