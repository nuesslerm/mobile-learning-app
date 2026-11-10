// The phone's own transport: the pinned mobile `DirectRpcClient` (pairing key, v2 e2ee handshake, request tracker) bundled for Node.
// Two imports are stubbed because they serve the app shell, not the wire: AsyncStorage (the host latch is kept in memory, one fresh
// phone per run) and the translation helper (its fallback text only reaches human-readable log lines).
import { existsSync } from 'node:fs'
import { bundleModule, type Stub } from '../bundle.ts'
import { assertAtPin, orcaRoot } from '../orca-lib/pin.ts'

export type RpcResponse = { id: string; ok: true; result: unknown; _meta?: unknown } | { id: string; ok: false; error: { code: string; message: string } }

export type PhoneClient = {
  sendRequest(method: string, params?: unknown, options?: { timeoutMs?: number }): Promise<RpcResponse>
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

let phone: Promise<PhoneModule> | undefined
let pairing: Promise<{ parsePairingCode(input: string): PairingOffer | null }> | undefined

export type PairingOffer = { v: number; endpoint: string; deviceToken: string; publicKeyB64: string; scope?: string }

/** The pinned parser of `orca://pair?code=...`: the offer an owner pairs a phone with, or an orcad prints when it is ready. */
export function loadPairing() {
  pairing ??= (async () => {
    assertAtPin()
    // Orca moved the parser from src/shared/pairing to the mobile transport (1.4.253); either pin must load.
    const moved = !existsSync(`${orcaRoot()}/src/shared/pairing.ts`)
    const module = moved ? 'mobile/src/transport/pairing' : 'src/shared/pairing'
    return bundleModule<{ parsePairingCode(input: string): PairingOffer | null }>(`export { parsePairingCode } from '${orcaRoot()}/${module}'\n`)
  })()
  return pairing
}

export function loadPhone(): Promise<PhoneModule> {
  phone ??= (async () => {
    assertAtPin()
    // The client logs progress with console.log; stdout carries the driver protocol.
    for (const level of ['log', 'info', 'warn', 'debug'] as const) console[level] = (...args: unknown[]) => void process.stderr.write(`${args.map(String).join(' ')}\n`)
    return bundleModule<PhoneModule>(`export { DirectRpcClient } from '${orcaRoot()}/mobile/src/transport/direct-rpc-client'\n`, undefined, STUBS)
  })()
  return phone
}
