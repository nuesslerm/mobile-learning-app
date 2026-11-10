// Ops over the pinned Orca paired-e2ee modules (v2 client session, desktop session, key schedule, legacy box framing, pairing offer).
// Every key, nonce and token is a case input (a fixed fill byte), never drawn from randomness, so a recording is reproducible.
import { bundleModule } from '../bundle.ts'
import { ret, type Json, type Op, type Reply } from '../protocol.ts'
import { assertAtPin, orcaRoot } from './pin.ts'

type Bytes = Uint8Array

type Lib = {
  nacl: {
    box: {
      keyPair: { fromSecretKey(sk: Bytes): { publicKey: Bytes; secretKey: Bytes } }
      before(pub: Bytes, sk: Bytes): Bytes
      after(msg: Bytes, nonce: Bytes, key: Bytes): Bytes
    }
  }
  PairedE2EEV2ClientSession: { create(o: any): any }
  P: unknown
  DesktopMobileE2EEV2Session: { create(o: any): any }
  createMobileE2EEV2Fixture(): any
  MOBILE_E2EE_V2_VECTOR: { transcriptLength: number }
  encodeMobileE2EEV2Transcript(hs: unknown): Bytes
  validateMobileE2EEV2Handshake(hello: unknown, ready: unknown): any
  deriveMobileE2EEV2KeySchedule(o: any): any
  pairedE2EEV2TokenAuth(hash: string, token: string): unknown
  encodePairingFixturePayload(offer: unknown): string
}

let lib: Promise<Lib> | undefined

function load(): Promise<Lib> {
  lib ??= (async () => {
    assertAtPin()
    const root = orcaRoot()
    const shared = `${root}/src/shared`
    return bundleModule<Lib>(`
import nacl from 'tweetnacl'
import { PairedE2EEV2ClientSession } from '${shared}/paired-e2ee-v2-client-session'
import { NODE_PAIRED_E2EE_PRIMITIVES as P } from '${shared}/paired-e2ee-node-primitives'
import { DesktopMobileE2EEV2Session } from '${root}/src/main/runtime/rpc/mobile-e2ee-v2-desktop-session'
import { createMobileE2EEV2Fixture, MOBILE_E2EE_V2_VECTOR } from '${shared}/mobile-e2ee-v2-fixtures'
import { encodeMobileE2EEV2Transcript, validateMobileE2EEV2Handshake } from '${shared}/mobile-e2ee-v2-contract'
import { deriveMobileE2EEV2KeySchedule } from '${shared}/mobile-e2ee-v2-node-key-schedule'
import { pairedE2EEV2TokenAuth } from '${shared}/paired-e2ee-client-handshake'
import { encodePairingFixturePayload } from '${shared}/mobile-relay-pairing-fixtures'
export { nacl, PairedE2EEV2ClientSession, P, DesktopMobileE2EEV2Session, createMobileE2EEV2Fixture, MOBILE_E2EE_V2_VECTOR }
export { encodeMobileE2EEV2Transcript, validateMobileE2EEV2Handshake, deriveMobileE2EEV2KeySchedule, pairedE2EEV2TokenAuth, encodePairingFixturePayload }
`)
  })()
  return lib
}

const hex = (b: Bytes) => Buffer.from(b).toString('hex')
const b64 = (b: Bytes) => Buffer.from(b).toString('base64')
const fill = (n: number, byte: string) => new Uint8Array(n).fill(parseInt(byte, 16))

type Fills = { clientSecretFill: string; serverSecretFill: string }

async function keys(i: Fills) {
  const { nacl } = await load()
  const clientSk = fill(32, i.clientSecretFill)
  const serverSk = fill(32, i.serverSecretFill)
  return { nacl, clientSk, serverSk, client: nacl.box.keyPair.fromSecretKey(clientSk), server: nacl.box.keyPair.fromSecretKey(serverSk) }
}

const keypair: Op = async (i: { secretFill: string }): Promise<Reply> => {
  const { nacl } = await load()
  const sk = fill(32, i.secretFill)
  return ret({ secretHex: hex(sk), publicHex: hex(nacl.box.keyPair.fromSecretKey(sk).publicKey) })
}

const fixedBytes: Op = (i: { fill: string; length: number }): Reply => ret({ hex: hex(fill(i.length, i.fill)) })

const boxSharedKey: Op = async (i: Fills): Promise<Reply> => {
  const { nacl, serverSk, client } = await keys(i)
  return ret({ hex: hex(nacl.box.before(client.publicKey, serverSk)) })
}

const pinnedV2Vector: Op = async (): Promise<Reply> => {
  const l = await load()
  const fx = l.createMobileE2EEV2Fixture()
  const hs = l.validateMobileE2EEV2Handshake(fx.hello, fx.ready)!
  const sched = l.deriveMobileE2EEV2KeySchedule({
    sharedSecret: fx.sharedSecret, transcript: l.encodeMobileE2EEV2Transcript(hs), clientNonce: hs.clientNonce, desktopNonce: hs.desktopNonce
  })
  return ret({
    hello: fx.hello, ready: fx.ready, sharedSecretHex: hex(fx.sharedSecret),
    transcriptLength: l.MOBILE_E2EE_V2_VECTOR.transcriptLength,
    transcriptHashHex: hex(sched.transcriptHash), mobileToDesktopKeyHex: hex(sched.mobileToDesktopKey),
    desktopToMobileKeyHex: hex(sched.desktopToMobileKey), sessionIdHex: hex(sched.sessionId)
  })
}

type HandshakeInput = Fills & {
  transport: 'direct' | 'relay'
  relayHostId?: string
  clientNonceFill: string
  desktopNonceFill: string
  deviceToken: string
  clientBinary: number[]
  serverBinary: number[]
}

/** A full v2 session between the pinned client and desktop code: hello, ready, an auth frame, its reply, and a binary frame each way.
 * Each side must open what the other sealed, or the op fails (the recording would be of a broken reference). */
const handshake: Op = async (i: HandshakeInput): Promise<Reply> => {
  const l = await load()
  const { client, server, serverSk } = await keys(i)
  const clientNonce = fill(32, i.clientNonceFill)
  const desktopNonce = fill(32, i.desktopNonceFill)
  const c = l.PairedE2EEV2ClientSession.create({
    primitives: l.P, desktopPublicKeyB64: b64(server.publicKey), transport: i.transport, relayHostId: i.relayHostId, clientKeyPair: client, clientNonce
  })
  const d = l.DesktopMobileE2EEV2Session.create({
    hello: c.hello, serverSecretKey: serverSk, expectedContext: { transport: i.transport, relayHostId: i.relayHostId }, randomBytes: () => desktopNonce
  })!
  if (!c.acceptReady(d.ready)) throw new Error('client refused ready')
  const authText = JSON.stringify(l.pairedE2EEV2TokenAuth(c.transcriptHashB64, i.deviceToken))
  const authFrame = c.sealText(authText)
  if (d.openText(authFrame) !== authText) throw new Error('desktop could not open auth')
  const authenticated = JSON.stringify({ type: 'e2ee_authenticated', v: 2, transcriptHashB64: d.transcriptHashB64 })
  const replyFrame = d.sealText(authenticated)
  if (c.openText(replyFrame) !== authenticated) throw new Error('client could not open reply')
  const clientBinary = new Uint8Array(i.clientBinary)
  const binFrame = c.sealBinary(clientBinary)
  if (hex(d.openBinary(binFrame)!) !== hex(clientBinary)) throw new Error('binary mismatch')
  const serverBinary = new Uint8Array(i.serverBinary)
  const serverBinFrame = d.sealBinary(serverBinary)
  if (hex(c.openBinary(serverBinFrame)!) !== hex(serverBinary)) throw new Error('binary mismatch 2')
  return ret({
    transport: i.transport, relayHostId: i.relayHostId ?? null,
    hello: c.hello, ready: d.ready, transcriptHashB64: d.transcriptHashB64,
    authText, authFrameB64: authFrame, authenticatedText: authenticated, replyFrameB64: replyFrame,
    clientBinaryHex: hex(clientBinary), clientBinaryFrameHex: hex(binFrame),
    serverBinaryHex: hex(serverBinary), serverBinaryFrameHex: hex(serverBinFrame)
  })
}

const legacy: Op = async (i: Fills & { nonceFill: string; plaintext: string }): Promise<Reply> => {
  const { nacl, serverSk, client } = await keys(i)
  const key = nacl.box.before(client.publicKey, serverSk)
  const nonce = fill(24, i.nonceFill)
  const box = nacl.box.after(new TextEncoder().encode(i.plaintext), nonce, key)
  const frame = new Uint8Array(24 + box.length)
  frame.set(nonce)
  frame.set(box, 24)
  return ret({ plaintext: i.plaintext, nonceHex: hex(nonce), frameHex: hex(frame), frameB64: b64(frame) })
}

/** The pairing offer: its fields are the input, except the desktop public key, which is derived from the server secret fill. The URI is
 * base64 of the JSON text, so the field order is part of the behaviour: the driver fixes it (a cassette's input is key-sorted). */
const offer: Op = async (i: { serverSecretFill: string; offer: Record<string, Json> }): Promise<Reply> => {
  const l = await load()
  const server = l.nacl.box.keyPair.fromSecretKey(fill(32, i.serverSecretFill))
  const o = i.offer
  const json = { v: o.v, endpoint: o.endpoint, deviceToken: o.deviceToken, publicKeyB64: b64(server.publicKey), pairedDeviceId: o.pairedDeviceId, scope: o.scope }
  return ret({ json, uri: l.encodePairingFixturePayload(json) })
}

export const ops: Record<string, Op> = {
  'crypto.keypair': keypair, 'crypto.fixed-bytes': fixedBytes, 'crypto.box-shared-key': boxSharedKey, 'crypto.pinned-v2-vector': pinnedV2Vector,
  'crypto.handshake': handshake, 'crypto.legacy': legacy, 'crypto.offer': offer
}
