// What the sidecar needs of the phone's client: the connect wait that reports a client state, the response without its per-run id, and the
// mask for per-run directories.
import { tmpdir } from 'node:os'
import type { Json } from '../protocol.ts'
import { type PhoneClient, type RpcResponse } from './phone.ts'

const FAILED = new Set(['auth-failed', 'disconnected'])

export function connected(client: PhoneClient, ms: number): Promise<string> {
  return new Promise((resolve) => {
    const done = (state: string) => {
      clearTimeout(timer)
      off()
      resolve(state)
    }
    const timer = setTimeout(() => done('timeout'), ms)
    const off = client.onStateChange((state) => {
      if (state === 'connected' || FAILED.has(state)) done(state)
    })
    if (client.getState() === 'connected') done('connected')
  })
}

/** The response as the host sent it, minus the id the client minted (`rpc-<n>-<epoch ms>`). */
export const withoutId = (response: RpcResponse): Json => {
  const { id: _id, ...rest } = response
  return rest as Json
}

/** A throwaway directory's name and orcad's own user data dir (`mimic-orcad-<6>`) differ per run: both are masked wherever they reach a frame. */
export const maskPaths = (v: Json): Json => {
  const root = tmpdir().replaceAll(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const re = new RegExp(`${root}/mimic-(orcad|scratch)-[A-Za-z0-9]{6}`, 'g')
  const swap = (x: Json): Json => {
    if (typeof x === 'string') return x.replace(re, (_m, kind: string) => (kind === 'orcad' ? '<userdata>' : '<scratch>'))
    if (Array.isArray(x)) return x.map(swap)
    if (x !== null && typeof x === 'object') return Object.fromEntries(Object.entries(x).map(([k, y]) => [k, swap(y)]))
    return x
  }
  return swap(v)
}
