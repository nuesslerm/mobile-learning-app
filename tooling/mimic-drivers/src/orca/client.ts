// Runs a script on the phone's own transport against one Orca host and reports the decrypted application frames: per request one
// `c2s` step (method, params) and one `s2c` step (the response without its per-run id). The e2ee ciphertext is not recorded:
// nonces make it differ per run, and the behaviour under test is what the host answers.
import { execFileSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import type { Json, Outcome, Reply, Step } from '../protocol.ts'
import { captured, fill, unbound, type Capture, type Vars } from '../capture.ts'
import { loadPhone, type PhoneClient, type RpcResponse } from './phone.ts'
import type { Target } from './target.ts'

export type ScriptRequest = { method: string; params?: Json; timeout_ms?: number; capture?: Capture }
/** `setup` makes git repositories before the first request, each a var `{git.<name>}` holding its path (a scenario's populated state). */
export type Script = { requests: ScriptRequest[]; connect_timeout_ms?: number; setup?: { git: string }[] }

const FAILED = new Set(['auth-failed', 'disconnected'])

function connected(client: PhoneClient, ms: number): Promise<string> {
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

const GIT_ENV = { GIT_AUTHOR_NAME: 'mimic', GIT_AUTHOR_EMAIL: 'mimic@example.invalid', GIT_COMMITTER_NAME: 'mimic', GIT_COMMITTER_EMAIL: 'mimic@example.invalid', GIT_AUTHOR_DATE: '2026-01-01T00:00:00Z', GIT_COMMITTER_DATE: '2026-01-01T00:00:00Z' }

/** One repository with one commit, the same bytes (so the same commit id) every time. */
export function makeGitRepo(path: string): void {
  mkdirSync(path, { recursive: true })
  writeFileSync(join(path, 'README.md'), '# mimic\n')
  const git = (...args: string[]) => execFileSync('git', ['-c', 'commit.gpgsign=false', '-C', path, ...args], { env: { PATH: process.env.PATH ?? '', HOME: path, ...GIT_ENV }, stdio: 'ignore' })
  git('init', '-q', '-b', 'main')
  git('add', '.')
  git('commit', '-q', '-m', 'mimic')
}

const SCRATCH = 'mimic-scratch-'

/** The variable names a script declares (captures and setup), the only placeholders whose absence is an error. */
export function declared(script: Script): Set<string> {
  return new Set([...(script.setup ?? []).map((s) => `git.${s.git}`), ...script.requests.flatMap((r) => Object.keys(r.capture ?? {}))])
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

export async function runScript(target: Target, script: Script): Promise<Reply> {
  const scratch = script.setup?.length ? mkdtempSync(join(tmpdir(), SCRATCH)) : undefined
  try {
    const vars: Vars = {}
    for (const s of script.setup ?? []) {
      makeGitRepo(join(scratch!, s.git))
      vars[`git.${s.git}`] = join(scratch!, s.git)
    }
    const reply = await drive(target, script, vars)
    return { ...reply, steps: reply.steps.map((s) => ({ ...s, payload: maskPaths(s.payload) })) }
  } finally {
    if (scratch) rmSync(scratch, { recursive: true, force: true })
  }
}

async function drive(target: Target, script: Script, vars: Vars): Promise<Reply> {
  const names = declared(script)
  const { DirectRpcClient } = await loadPhone()
  const client = new DirectRpcClient(target.endpoint, target.deviceToken, target.serverPublicKeyB64, {})
  try {
    const state = await connected(client, script.connect_timeout_ms ?? 10_000)
    if (state !== 'connected') return { steps: [], effects: [], outcome: { status: 'error', code: state } satisfies Outcome }
    const steps: Step[] = []
    for (const [i, request] of script.requests.entries()) {
      const rid = String(i)
      const params = fill(request.params ?? null, vars)
      steps.push({ dir: 'c2s', kind: 'json', rid, payload: { method: request.method, params } })
      let payload: Json
      const missing = unbound(params, vars, names)
      if (missing !== undefined) {
        // an earlier answer did not carry the value: that is the finding, and the request is not sent with a placeholder in it
        payload = { client_error: `unbound variable ${missing}` }
      } else {
        try {
          payload = withoutId(await client.sendRequest(request.method, params ?? undefined, { timeoutMs: request.timeout_ms ?? 10_000 }))
          if (request.capture) Object.assign(vars, captured(payload, request.capture))
        } catch (e) {
          payload = { client_error: (e as Error).message }
        }
      }
      steps.push({ dir: 's2c', kind: 'json', rid, payload })
    }
    return { steps, effects: [], outcome: { status: 'ok' } }
  } finally {
    client.close()
  }
}
