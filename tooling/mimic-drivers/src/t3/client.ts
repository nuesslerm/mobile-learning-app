// The client side of the T3 goldens, run against a server: an interaction's `script` (HTTP calls and a WebSocket session) is executed and the
// wire reported raw as steps, in the dialect of `wf mimic` taps (http call and return, ws:open, ws frames, ws:close); the set's rules mask it.
// `{credential}`, `{bad-credential}`, `{access_token}`, `{ticket}` and `{workspace}` in a script are the values of this run.
import type { Json, Step } from '../protocol.ts'
import { captured, fill, unbound, type Capture } from '../capture.ts'

export { fill }

export type Vars = Record<string, Json>
/** An entry is one op (`http`, `ws-open`, `ws-send`, `ws-close`); `capture` beside it names what its answer teaches the entries after it. */
export type Script = Record<string, Json>[]

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

const urlencode = (o: Record<string, Json>) => new URLSearchParams(Object.entries(o).map(([k, v]) => [k, String(v)])).toString()

function bodyText(body: Json, contentType: string): string | null {
  if (body === null || body === undefined) return null
  if (typeof body === 'string') return body
  return contentType.includes('x-www-form-urlencoded') ? urlencode(body as Record<string, Json>) : JSON.stringify(body)
}

const rid = (frame: Json): string | undefined => {
  const f = frame as { id?: unknown; requestId?: unknown } | null
  const id = f?.id ?? f?.requestId
  return typeof id === 'string' || typeof id === 'number' ? String(id) : undefined
}

function frameStep(dir: 'c2s' | 's2c', text: string): Step {
  try {
    const payload = JSON.parse(text) as Json
    const r = rid(payload)
    return { dir, channel: 'ws', kind: 'json', ...(r === undefined ? {} : { rid: r }), payload }
  } catch {
    return { dir, channel: 'ws', kind: 'text', payload: text }
  }
}

export async function runScript(base: string, script: Script, vars: Vars, settleMs: number): Promise<Step[]> {
  const steps: Step[] = []
  let ws: WebSocket | undefined
  let lastFrameAt = Date.now()
  const send = (text: string) => {
    steps.push(frameStep('c2s', text))
    ws!.send(text)
  }
  // everything the server sends until it has been quiet for `ms`
  const quiet = async (ms: number) => {
    do await sleep(Math.min(ms, 25))
    while (Date.now() - lastFrameAt < ms)
  }
  // a server that drops the connection is a recorded behaviour (the interaction ends there), not a driver failure
  try {
    const names = new Set(script.flatMap((e) => Object.keys((e.capture ?? {}) as object)))
    for (const entry of script) {
      const [op, raw] = Object.entries(entry)[0]!
      const arg = fill(raw!, vars) as Record<string, Json>
      const capture = entry.capture as Capture | undefined
      const missing = unbound(arg, vars, names)
      if (missing !== undefined) {
        // an earlier answer did not carry the value: that is the finding, and the entry is not sent with a placeholder in it
        steps.push({ dir: 'ret', channel: 'driver', kind: 'json', payload: { client_error: `unbound variable ${missing}`, op } })
        continue
      }
      if (op === 'http') {
        const headers = (arg.headers ?? {}) as Record<string, string>
        const type = Object.entries(headers).find(([k]) => k.toLowerCase() === 'content-type')?.[1] ?? ''
        const body = bodyText(arg.body ?? null, type)
        const path = String(arg.path)
        const method = String(arg.method ?? 'GET')
        const res = await fetch(new URL(path, base), { method, headers, body: body ?? undefined, redirect: 'manual' })
        const text = await res.text()
        steps.push({ dir: 'call', channel: 'http', kind: 'json', payload: { method, path, headers, body } })
        steps.push({ dir: 'ret', channel: 'http', kind: 'json', payload: { status: res.status, headers: Object.fromEntries(res.headers), body: text === '' ? null : text } })
        try {
          const parsed = JSON.parse(text) as { access_token?: unknown; ticket?: unknown }
          if (capture) Object.assign(vars, captured(parsed as Json, capture))
          if (typeof parsed.access_token === 'string') vars.access_token = parsed.access_token
          if (typeof parsed.ticket === 'string') vars.ticket = parsed.ticket
        } catch {
          // an answer that is not JSON teaches no variable
        }
      } else if (op === 'ws-open') {
        const path = String(arg.path)
        const socket = new WebSocket(new URL(path, base.replace(/^http/, 'ws')))
        await new Promise<void>((resolve, reject) => {
          socket.onopen = () => resolve()
          socket.onerror = () => reject(new Error('websocket refused'))
        })
        socket.onmessage = (e) => {
          lastFrameAt = Date.now()
          const text = String(e.data)
          steps.push(frameStep('s2c', text))
          const frame = frameStep('s2c', text).payload as { _tag?: string; requestId?: Json }
          if (frame?._tag === 'Chunk') send(JSON.stringify({ _tag: 'Ack', requestId: frame.requestId }))
        }
        ws = socket
        steps.push({ dir: 's2c', channel: 'ws:open', kind: 'json', payload: { path, status: 101 } })
      } else if (op === 'ws-send') {
        const from = steps.length
        send(JSON.stringify(arg))
        lastFrameAt = Date.now()
        // a setup request is not over until its Exit has come: a loaded server answers after the quiet period and the frame then lands in a later step
        if (entry.await === 'exit') {
          const answered = () => steps.slice(from).some((x) => x.dir === 's2c' && (x.payload as { _tag?: string; requestId?: Json })?._tag === 'Exit' && String((x.payload as { requestId?: Json }).requestId) === String(arg.id))
          for (let waited = 0; !answered() && waited < 15000; waited += 25) await sleep(25)
        }
        await quiet(settleMs)
        if (capture) {
          // the answer is the request's Exit frame (a stream's Chunk frames carry no final value)
          const exit = steps.slice(from).find((x) => x.dir === 's2c' && (x.payload as { _tag?: string; requestId?: Json })?._tag === 'Exit' && String((x.payload as { requestId?: Json }).requestId) === String(arg.id))
          if (exit) Object.assign(vars, captured(exit.payload, capture))
        }
      } else if (op === 'ws-close') {
        const code = Number(arg.code ?? 1000)
        const reason = String(arg.reason ?? '')
        const closed = new Promise<CloseEvent>((resolve) => ws!.addEventListener('close', resolve, { once: true }))
        steps.push({ dir: 'c2s', channel: 'ws:close', kind: 'json', payload: { code, reason, tcp: false } })
        ws!.close(code, reason)
        const by = await Promise.race([closed, sleep(2000).then(() => undefined)])
        if (by) steps.push({ dir: 's2c', channel: 'ws:close', kind: 'json', payload: { code: by.code, reason: by.reason, tcp: false } })
        steps.push({ dir: 's2c', channel: 'ws:close', kind: 'json', payload: { code: null, reason: '', tcp: true } })
      } else {
        throw new Error(`unknown script step ${op}`)
      }
    }
  } catch (e) {
    const cause = (e as { cause?: { code?: string } }).cause?.code
    if (/^unknown script step/.test((e as Error).message)) throw e
    steps.push({ dir: 'ret', channel: 'driver', kind: 'json', payload: { transport_error: cause ?? (e as Error).message } })
  }
  return steps
}

export function withoutWorkspace(steps: Step[], workspace: string): Step[] {
  const swap = (v: Json): Json => {
    if (typeof v === 'string') return v.replaceAll(workspace, '<workspace>')
    if (Array.isArray(v)) return v.map(swap)
    if (v !== null && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, swap(x)]))
    return v
  }
  return steps.map((s) => ({ ...s, payload: swap(s.payload) }))
}
