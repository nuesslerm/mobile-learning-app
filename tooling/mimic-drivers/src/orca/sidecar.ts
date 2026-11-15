// The Orca codec sidecar of the step interpreter: the phone's own pinned `DirectRpcClient` (keys, e2ee handshake, request tracker) behind one
// JSON object per line on stdio (docs/plans/mimic/orca-phone-protocol.md). The host comes from the environment as for the `script` driver
// (target.ts), or is an orcad provisioned at the pin. Control lines carry an `id` and are answered `{id, ok}` or `{id, error}`; frames of an open
// stream and the end of the connection are unsolicited `{event}` lines in arrival order. Every decision (windows, ordering, masking, step
// shapes) is the Rust interpreter's; this process only reports what crossed the client.
import { createInterface } from 'node:readline'
import type { Json } from '../protocol.ts'
import { toolchainLockHash } from '../lock.ts'
import { readPin } from '../orca-lib/pin.ts'
import { shared, type Shared } from '../session.ts'
import { connected, maskPaths, withoutId } from './client.ts'
import { launcher, type Launch } from './index.ts'
import { loadPhone, type PhoneClient, type PhoneModule } from './phone.ts'
import type { Target } from './target.ts'

export type Control =
  | { op: 'hello' }
  | { op: 'reset' }
  | { op: 'tap'; input?: { handshake?: boolean; close?: boolean } }
  | { op: 'connect'; input?: { timeout_ms?: number } }
  | { op: 'request'; input: { method: string; params?: Json; timeout_ms?: number } }
  | { op: 'subscribe'; input: { method: string; params?: Json } }
  | { op: 'unsubscribe'; input: { sub: string } }
  | { op: 'close' }
  | { op: 'quit' }

export type Message = Control & { id?: string }

export type Deps = { launch: () => Promise<Launch>; load: () => Promise<PhoneModule>; emit: (line: Json) => void }

/** Bytes inside a decoded frame become base64, so a frame is always JSON. */
export function jsonSafe(value: unknown): Json {
  if (value instanceof Uint8Array) return Buffer.from(value).toString('base64')
  if (Array.isArray(value)) return value.map(jsonSafe)
  if (value !== null && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, jsonSafe(v)]))
  return (value ?? null) as Json
}

export function sidecar(deps: Deps) {
  let held: Shared<Target> | undefined
  let client: PhoneClient | undefined
  const disposers = new Map<string, () => void>()
  let counter = 0
  let wantHandshake = false
  let wantClose = false
  const target = async () => {
    held ??= shared<Target>((use) => deps.launch().then((launch) => launch(use)))
    return (await held.get()).server
  }
  const drop = () => {
    for (const dispose of disposers.values()) dispose()
    disposers.clear()
    client?.close()
    client = undefined
  }
  return {
    async handle(message: Message): Promise<Json> {
      switch (message.op) {
        case 'hello':
          return { hello: { name: 'mimic-orca-phone', protocol: 'mimic-orca-phone/1', target_version: readPin().version, toolchain_lock_hash: toolchainLockHash() } }
        case 'reset':
          drop()
          wantHandshake = false
          wantClose = false
          return {}
        case 'tap':
          wantHandshake = message.input?.handshake === true
          wantClose = message.input?.close === true
          return {}
        case 'connect': {
          drop()
          const { DirectRpcClient } = await deps.load()
          const host = await target()
          client = new DirectRpcClient(host.endpoint, host.deviceToken, host.serverPublicKeyB64, {})
          return { state: await connected(client, message.input?.timeout_ms ?? 10_000) }
        }
        case 'request': {
          if (!client) return { client_error: 'not connected' }
          try {
            const { method, params, timeout_ms } = message.input
            return { response: maskPaths(withoutId(await client.sendRequest(method, params ?? undefined, { timeoutMs: timeout_ms ?? 10_000 }))) }
          } catch (e) {
            return { client_error: (e as Error).message }
          }
        }
        case 'subscribe': {
          if (!client) return { client_error: 'not connected' }
          const sub = `s${counter++}`
          const { method, params } = message.input
          const frame = (payload: unknown) => deps.emit({ event: 'frame', sub, kind: 'json', payload: maskPaths(jsonSafe(payload)) })
          disposers.set(sub, client.subscribe(method, params ?? null, frame, { onBinaryFrame: frame }))
          return { sub }
        }
        case 'unsubscribe': {
          disposers.get(message.input.sub)?.()
          disposers.delete(message.input.sub)
          return {}
        }
        case 'close':
          drop()
          if (wantClose) deps.emit({ event: 'close', code: null, reason: '' })
          return { state: 'disconnected' }
        case 'quit':
          drop()
          await held?.close()
          held = undefined
          return {}
      }
    },
    get handshake() {
      return wantHandshake
    }
  }
}

/** Reads one control line at a time and answers each; the process ends after `quit` or when the input does. */
export async function serveSidecar(input: NodeJS.ReadableStream, write: (line: string) => void, deps: Partial<Deps> = {}): Promise<void> {
  const emit = (line: Json) => write(JSON.stringify(line))
  const side = sidecar({ launch: () => launcher(process.env), load: loadPhone, emit, ...deps })
  for await (const line of createInterface({ input })) {
    if (!line.trim()) continue
    const message = JSON.parse(line) as Message
    try {
      emit({ id: message.id ?? null, ok: await side.handle(message) })
    } catch (e) {
      emit({ id: message.id ?? null, error: (e as Error).message })
    }
    if (message.op === 'quit') return
  }
  await side.handle({ op: 'quit' })
}
