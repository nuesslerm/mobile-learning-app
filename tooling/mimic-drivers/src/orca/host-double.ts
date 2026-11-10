// A scripted double of an Orca host: the pinned desktop `E2EEChannel` (hello, ready, token auth, sealed frames) behind a minimal
// WebSocket server, answering requests from a method table instead of a runtime. It stands in for orcad where orcad cannot run
// (flow tests, an arm64 sandbox), so a recording made against it is labelled `authored`, never `recorded`.
// `node src/orca/host-double.ts` prints the ready line an orcad prints (`orca_server_ready` with a pairing url) and serves until stdin
// closes; ORCA_DOUBLE_METHODS (a JSON method table) says what it answers.
import { createHash } from 'node:crypto'
import { createServer, type Server } from 'node:http'
import type { Socket } from 'node:net'
import { bundleModule } from '../bundle.ts'
import { assertAtPin, orcaRoot } from '../orca-lib/pin.ts'
import type { Json } from '../protocol.ts'

type Bytes = Uint8Array

type Channel = {
  handleRawMessage(raw: string | Bytes): void
  onMessage(h: (plaintext: string, reply: (response: string) => void) => void): void
}

type Lib = {
  nacl: { box: { keyPair: { fromSecretKey(sk: Bytes): { publicKey: Bytes } } } }
  E2EEChannel: new (ws: unknown, o: Record<string, unknown>) => Channel
}

let lib: Promise<Lib> | undefined

function load(): Promise<Lib> {
  lib ??= (async () => {
    assertAtPin()
    return bundleModule<Lib>(`
import nacl from 'tweetnacl'
import { E2EEChannel } from '${orcaRoot()}/src/main/runtime/rpc/e2ee-channel'
export { nacl, E2EEChannel }
`, undefined, [{ filter: /^posthog-node$/, contents: 'module.exports = { PostHog: class { capture() {} shutdown() {} } }' }])
  })()
  return lib
}

/** What the double answers: a result per method, or an error `{code, message}`. A method not listed is `method_not_found`. */
type Outcome = { result: Json } | { error: { code: string; message: string } }

/** A method's answer: one outcome, or a list tried in order where `when` (params it must carry, equal) picks the first that fits. */
export type Methods = Record<string, Outcome | ({ when?: Record<string, Json> } & Outcome)[]>

const carries = (params: unknown, when: Record<string, Json> = {}) =>
  Object.entries(when).every(([k, v]) => JSON.stringify((params as Record<string, unknown> | undefined)?.[k]) === JSON.stringify(v))

export type HostDouble = {
  endpoint: string
  deviceToken: string
  serverPublicKeyB64: string
  /** The method names the double was asked, in order. */
  asked: string[]
  /** The same requests with the params they carried. */
  calls: { method: string; params: unknown }[]
  close(): Promise<void>
}

export type DoubleOptions = { serverSecretFill?: string; deviceToken?: string; methods: Methods; runtimeId?: string }

const CAPABILITY_UPDATE = 'runtime.clientCapabilities.update'
const GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11'

function frame(opcode: number, payload: Buffer): Buffer {
  const n = payload.length
  const head = n < 126 ? Buffer.from([0x80 | opcode, n]) : n < 65536 ? Buffer.from([0x80 | opcode, 126, n >> 8, n & 255]) : Buffer.concat([Buffer.from([0x80 | opcode, 127]), Buffer.alloc(8)])
  if (n >= 65536) head.writeBigUInt64BE(BigInt(n), 2)
  return Buffer.concat([head, payload])
}

/** The client frames in `buf`: masked, possibly fragmented across reads; returns the rest. */
function parseFrames(buf: Buffer, onFrame: (opcode: number, payload: Buffer) => void): Buffer {
  for (;;) {
    if (buf.length < 2) return buf
    let len = buf[1]! & 0x7f
    let at = 2
    if (len === 126) {
      if (buf.length < 4) return buf
      len = buf.readUInt16BE(2)
      at = 4
    } else if (len === 127) {
      if (buf.length < 10) return buf
      len = Number(buf.readBigUInt64BE(2))
      at = 10
    }
    if (buf.length < at + 4 + len) return buf
    const mask = buf.subarray(at, at + 4)
    const payload = Buffer.from(buf.subarray(at + 4, at + 4 + len))
    for (let i = 0; i < len; i++) payload[i] = payload[i]! ^ mask[i % 4]!
    onFrame(buf[0]! & 0x0f, payload)
    buf = buf.subarray(at + 4 + len)
  }
}

// The slice of the `ws` socket the channel uses.
function socketOf(sock: Socket) {
  const ws = {
    OPEN: 1,
    readyState: 1,
    bufferedAmount: 0,
    send(data: string | Bytes) {
      if (!sock.destroyed) sock.write(typeof data === 'string' ? frame(1, Buffer.from(data)) : frame(2, Buffer.from(data)))
    },
    close(code = 1000) {
      ws.readyState = 3
      if (!sock.destroyed) sock.end(frame(8, Buffer.from([code >> 8, code & 255])))
    }
  }
  return ws
}

export async function startHostDouble(options: DoubleOptions): Promise<HostDouble> {
  const { nacl, E2EEChannel } = await load()
  const secret = new Uint8Array(32).fill(parseInt(options.serverSecretFill ?? '22', 16))
  const publicKey = nacl.box.keyPair.fromSecretKey(secret).publicKey
  const deviceToken = options.deviceToken ?? 'orca-double-device-token'
  const runtimeId = options.runtimeId ?? 'runtime-double'
  const asked: string[] = []
  const calls: { method: string; params: unknown }[] = []
  const principal = { deviceId: 'paired-device', scope: 'mobile', clientId: deviceToken, callerFingerprint: createHash('sha256').update(deviceToken).digest('hex'), credentialRevision: 0, deviceToken }

  const server: Server = createServer()
  const sockets = new Set<Socket>()
  server.on('upgrade', (req, sock: Socket) => {
    sockets.add(sock)
    sock.on('close', () => sockets.delete(sock))
    sock.on('error', () => undefined)
    const accept = createHash('sha1').update(`${req.headers['sec-websocket-key']}${GUID}`).digest('base64')
    sock.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`)
    const ws = socketOf(sock)
    const channel = new E2EEChannel(ws, {
      serverSecretKey: secret,
      resolveAuthenticatedDevice: (token: string) => (token === deviceToken ? principal : null),
      onReady: (ch: Channel) =>
        ch.onMessage((plaintext, reply) => {
          const request = JSON.parse(plaintext) as { id: string; method: string; params?: unknown }
          asked.push(request.method)
          calls.push({ method: request.method, params: request.params })
          const listed = options.methods[request.method]
          const entry = (Array.isArray(listed) ? listed.find((o) => carries(request.params, o.when)) : listed) ?? (request.method === CAPABILITY_UPDATE ? { result: { ok: true } } : undefined)
          const outcome = entry ?? { error: { code: 'method_not_found', message: `Unknown method: ${request.method}` } }
          reply(JSON.stringify('result' in outcome ? { id: request.id, ok: true, result: outcome.result, _meta: { runtimeId } } : { id: request.id, ok: false, error: outcome.error, _meta: { runtimeId } }))
        }),
      onError: (code: number) => ws.close(code)
    })
    let pending: Buffer = Buffer.alloc(0)
    sock.on('data', (d) => {
      pending = parseFrames(Buffer.concat([pending, d]), (opcode, payload) => {
        if (opcode === 1) channel.handleRawMessage(payload.toString('utf8'))
        else if (opcode === 2) channel.handleRawMessage(new Uint8Array(payload))
        else if (opcode === 8) ws.close()
      })
    })
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const { port } = server.address() as { port: number }
  return {
    endpoint: `ws://127.0.0.1:${port}`,
    deviceToken,
    serverPublicKeyB64: Buffer.from(publicKey).toString('base64'),
    asked,
    calls,
    close: () =>
      new Promise<void>((resolve) => {
        for (const s of sockets) s.destroy()
        server.close(() => resolve())
      })
  }
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  const methods: Methods = process.env.ORCA_DOUBLE_METHODS ? JSON.parse(process.env.ORCA_DOUBLE_METHODS) : {}
  const double = await startHostDouble({ methods })
  const offer = { v: 2, endpoint: double.endpoint, deviceToken: double.deviceToken, publicKeyB64: double.serverPublicKeyB64 }
  const url = `orca://pair?code=${Buffer.from(JSON.stringify(offer)).toString('base64url')}`
  process.stdout.write(`${JSON.stringify({ type: 'orca_server_ready', advertisedEndpoint: double.endpoint, pairing: { url } })}\n`)
  process.stdin.resume()
  process.stdin.on('end', () => void double.close().then(() => process.exit(0)))
}
