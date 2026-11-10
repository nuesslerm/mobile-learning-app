// A stand-in T3 server: the HTTP routes the driver calls and a WebSocket that answers frames the way the Effect RPC framing does.
import { createHash } from 'node:crypto'
import { createServer, type Server } from 'node:http'
import type { Socket } from 'node:net'

const GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11'

function frame(opcode: number, payload: Buffer): Buffer {
  return Buffer.concat([Buffer.from([0x80 | opcode, payload.length]), payload])
}

function unmask(buf: Buffer): { opcode: number; payload: Buffer; used: number } | undefined {
  if (buf.length < 6) return undefined
  let len = buf[1]! & 0x7f
  let at = 2
  if (len === 126) {
    if (buf.length < 8) return undefined
    len = buf.readUInt16BE(2)
    at = 4
  }
  if (buf.length < at + 4 + len) return undefined
  const key = buf.subarray(at, at + 4)
  const payload = Buffer.from(buf.subarray(at + 4, at + 4 + len).map((b, i) => b ^ key[i % 4]!))
  return { opcode: buf[0]! & 0x0f, payload, used: at + 4 + len }
}

export type Fake = { url: string; server: Server; close: () => Promise<void>; minted: string[]; sockets: Socket[] }

export async function fakeT3(): Promise<Fake> {
  const minted: string[] = []
  const sockets: Socket[] = []
  const server = createServer((req, res) => {
    let body = ''
    req.on('data', (c) => (body += c))
    req.on('end', () => {
      const send = (status: number, doc: unknown) => {
        res.writeHead(status, { 'content-type': 'application/json' })
        res.end(JSON.stringify(doc))
      }
      if (req.url === '/.well-known/t3/environment') return send(200, { environmentId: 'fake' })
      if (req.url === '/oauth/token') {
        const given = new URLSearchParams(body).get('subject_token')
        return given === minted.at(-1) ? send(200, { access_token: 'access-token-1', scope: 'a b' }) : send(401, { _tag: 'EnvironmentAuthInvalidError' })
      }
      if (req.url === '/api/auth/websocket-ticket/rpc') return send(req.headers.authorization === 'Bearer access-token-1' ? 200 : 401, { ticket: 'ticket-0001' })
      send(404, {})
    })
  })
  server.on('upgrade', (req, socket: Socket) => {
    sockets.push(socket)
    const key = String(req.headers['sec-websocket-key'])
    socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${createHash('sha1').update(key + GUID).digest('base64')}\r\n\r\n`)
    let pending = Buffer.alloc(0)
    socket.on('data', (chunk) => {
      pending = Buffer.concat([pending, chunk])
      for (let f = unmask(pending); f; f = unmask(pending)) {
        pending = pending.subarray(f.used)
        if (f.opcode === 8) {
          socket.write(frame(8, f.payload))
          socket.end()
          continue
        }
        const msg = JSON.parse(f.payload.toString()) as { _tag: string; id?: string }
        const out = (o: unknown) => socket.write(frame(1, Buffer.from(JSON.stringify(o))))
        if (msg._tag === 'Ping') out({ _tag: 'Pong' })
        else if (msg._tag === 'Request' && (msg as { tag?: string }).tag === 'test.slow') setTimeout(() => out({ _tag: 'Exit', requestId: msg.id, exit: { _tag: 'Success', value: {} } }), 150)
        else if (msg._tag === 'Request' && (msg as { tag?: string }).tag === 'test.value') out({ _tag: 'Exit', requestId: msg.id, exit: { _tag: 'Success', value: { runs: [{ runId: 'run-7' }] } } })
        else if (msg._tag === 'Request') out(msg.id === '1' ? { _tag: 'Chunk', requestId: '1', values: [{ kind: 'snapshot' }] } : { _tag: 'Exit', requestId: msg.id, exit: { _tag: 'Success', value: {} } })
      }
    })
  })
  await new Promise<void>((r) => server.listen(0, '127.0.0.1', r))
  const { port } = server.address() as { port: number }
  return { url: `http://127.0.0.1:${port}`, server, minted, sockets, close: () => new Promise((r) => { sockets.forEach((s) => s.destroy()); server.close(() => r()) }) }
}

