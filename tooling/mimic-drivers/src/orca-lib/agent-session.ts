// Ops over the pinned Orca agent-session wire code: the body fingerprint a queued card is compared by (`agentSessionSendBodyFingerprint`).
import { bundleModule } from '../bundle.ts'
import { ret, type Json, type Op, type Reply } from '../protocol.ts'
import { assertAtPin, orcaRoot } from './pin.ts'

type Lib = { agentSessionSendBodyFingerprint(sessionId: string, body: unknown): string }

let lib: Promise<Lib> | undefined

function load(): Promise<Lib> {
  lib ??= (async () => {
    assertAtPin()
    return bundleModule<Lib>(`export { agentSessionSendBodyFingerprint } from '${orcaRoot()}/src/shared/agent-session/requests/structured-agent-session-send-mutation'\n`)
  })()
  return lib
}

/** The fingerprint of a user message holding `texts` as its text blocks, in session `sessionId`. */
const bodyFingerprint: Op = async (i: { sessionId: string; texts: string[] }): Promise<Reply> => {
  const { agentSessionSendBodyFingerprint } = await load()
  const body = { kind: 'message', role: 'user', blocks: i.texts.map((text) => ({ type: 'text', text })) }
  return ret({ fingerprint: agentSessionSendBodyFingerprint(i.sessionId, body) } as Json)
}

export const ops: Record<string, Op> = { 'agent-session.body-fingerprint': bodyFingerprint }
