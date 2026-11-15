// The Orca codec sidecar of the step interpreter: the phone's own pinned `DirectRpcClient` (keys, e2ee handshake, request tracker) behind one
// JSON object per line on stdio, so the Rust interpreter records and replays Orca through the real client code. The host comes from the
// environment as for the `script` driver (target.ts), or is an orcad provisioned at the pin. Ops: `connect {timeout_ms}` -> `{state}`;
// `request {method, params, timeout_ms}` -> `{ok: true, payload}` (the host's answer without its per-run id, run paths masked) or
// `{ok: false, error}` (the client's own failure); `disconnect` ends the phone session; `close` stops the host and ends the process.
import { createInterface } from 'node:readline'
import type { Json } from '../protocol.ts'
import { shared, type Shared } from '../session.ts'
import { connected, maskPaths, withoutId } from './client.ts'
import { launcher, type Launch } from './index.ts'
import { loadPhone, type PhoneClient, type PhoneModule } from './phone.ts'
import type { Target } from './target.ts'

export type Message = { op: 'connect'; timeout_ms?: number } | { op: 'request'; method: string; params?: Json; timeout_ms?: number } | { op: 'disconnect' } | { op: 'close' }

export type Deps = { launch: () => Promise<Launch>; load: () => Promise<PhoneModule> }

export function sidecar(deps: Deps) {
  let held: Shared<Target> | undefined
  let client: PhoneClient | undefined
  const target = async () => {
    held ??= shared<Target>((use) => deps.launch().then((launch) => launch(use)))
    return (await held.get()).server
  }
  return {
    async handle(message: Message): Promise<Json> {
      switch (message.op) {
        case 'connect': {
          client?.close()
          const { DirectRpcClient } = await deps.load()
          const host = await target()
          client = new DirectRpcClient(host.endpoint, host.deviceToken, host.serverPublicKeyB64, {})
          return { state: await connected(client, message.timeout_ms ?? 10_000) }
        }
        case 'request': {
          if (!client) return { ok: false, error: 'not connected' }
          try {
            return { ok: true, payload: maskPaths(withoutId(await client.sendRequest(message.method, message.params ?? undefined, { timeoutMs: message.timeout_ms ?? 10_000 }))) }
          } catch (e) {
            return { ok: false, error: (e as Error).message }
          }
        }
        case 'disconnect':
          client?.close()
          client = undefined
          return {}
        case 'close':
          client?.close()
          client = undefined
          await held?.close()
          held = undefined
          return {}
      }
    }
  }
}

/** Reads messages from `input`, one per line, and writes one answer line each; the process ends after `close` or when the input does. */
export async function serveSidecar(input: NodeJS.ReadableStream, write: (line: string) => void, deps: Deps = { launch: () => launcher(process.env), load: loadPhone }): Promise<void> {
  const side = sidecar(deps)
  for await (const line of createInterface({ input })) {
    if (!line.trim()) continue
    const message = JSON.parse(line) as Message
    write(JSON.stringify(await side.handle(message)))
    if (message.op === 'close') return
  }
  await side.handle({ op: 'close' })
}
