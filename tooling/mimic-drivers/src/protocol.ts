// mimic-driver/1: one JSON object per line on stdin and stdout. {"hello":true} answers {"hello":{driver,target_version,coverage}};
// {"id","op","input"} answers {"id","steps","effects","outcome"} or {"id","error"} (the op could not run at all). {"reset":true} brings the
// target back to its initial state and answers {"reset":"ok"} (a driver with no state to reset answers ok too). With MIMIC_DRIVER_CONCURRENCY
// above 1 requests are answered as they finish, in any order, up to that many at once; hello and reset wait for the ones in flight.
import { createInterface } from 'node:readline'

export type Json = null | boolean | number | string | Json[] | { [key: string]: Json }

export type Step = {
  dir: 'c2s' | 's2c' | 'call' | 'ret' | 'stdin' | 'stdout' | 'stderr'
  channel?: string
  kind: 'text' | 'binary' | 'json' | 'exit'
  rid?: string
  len?: number
  payload: Json
}

export type Outcome = { status: 'ok' | 'error'; code?: string; reference_accepts?: boolean }

export type Reply = { steps: Step[]; effects?: Json[]; outcome?: Outcome }

export type Op = (input: any) => Promise<Reply> | Reply

export type Driver = {
  name: string
  version: string
  protocol: 'mimic-driver/1'
  toolchainLockHash?: string
  targetVersion?: string
  ops: Record<string, Op>
  reset?: () => Promise<void>
  /** Stops what the driver started (a real server); called when the input ends. */
  close?: () => Promise<void>
}

/** A one-step JSON reply: the shape nearly every op of a node-fn driver has. */
export function ret(payload: Json, outcome: Outcome = { status: 'ok', reference_accepts: true }): Reply {
  return { steps: [{ dir: 'ret', kind: 'json', payload }], effects: [], outcome }
}

export async function answer(driver: Driver, line: string): Promise<Json> {
  let request: any
  try {
    request = JSON.parse(line)
  } catch (e) {
    return { id: null, error: `not JSON: ${(e as Error).message}` }
  }
  if (request && request.reset !== undefined) {
    try {
      await driver.reset?.()
      return { reset: 'ok' }
    } catch (e) {
      return { id: null, error: `reset: ${(e as Error).message}` }
    }
  }
  if (request && request.hello !== undefined) {
    const info: Json = { name: driver.name, version: driver.version, protocol: driver.protocol }
    if (driver.toolchainLockHash) (info as any).toolchain_lock_hash = driver.toolchainLockHash
    const hello: Json = { driver: info, coverage: false }
    if (driver.targetVersion) (hello as any).target_version = driver.targetVersion
    return { hello }
  }
  const id = typeof request?.id === 'string' ? request.id : null
  const op = request?.op
  if (id === null || typeof op !== 'string') return { id, error: 'not a request: needs a string id and a string op' }
  const run = driver.ops[op]
  if (!run) return { id, error: `unknown op ${op} (this driver has: ${Object.keys(driver.ops).join(', ')})` }
  try {
    const reply = await run(request.input)
    return { id, steps: reply.steps as Json, effects: (reply.effects ?? []) as Json, outcome: (reply.outcome ?? { status: 'ok' }) as Json }
  } catch (e) {
    return { id, error: `${op}: ${(e as Error).message}` }
  }
}

const isControl = (line: string) => /^\s*\{\s*"(hello|reset)"/.test(line)

export async function serve(driver: Driver, input: NodeJS.ReadableStream, output: NodeJS.WritableStream): Promise<void> {
  const limit = Math.max(1, Number(process.env.MIMIC_DRIVER_CONCURRENCY ?? 1) || 1)
  const inflight = new Set<Promise<void>>()
  const write = (reply: Json) => void output.write(JSON.stringify(reply) + '\n')
  for await (const line of createInterface({ input })) {
    if (line.trim() === '') continue
    if (limit === 1 || isControl(line)) {
      await Promise.all(inflight)
      write(await answer(driver, line))
      continue
    }
    while (inflight.size >= limit) await Promise.race(inflight)
    const p: Promise<void> = answer(driver, line).then(write).finally(() => inflight.delete(p))
    inflight.add(p)
  }
  await Promise.all(inflight)
  await driver.close?.()
}
