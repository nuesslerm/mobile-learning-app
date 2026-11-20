export const PROTOCOL_VERSION = 1;

export type RpcErrorCode = 'timeout' | 'closed' | 'remote' | 'protocol' | 'unknown-method';

export type RpcErrorPayload = { code: RpcErrorCode; message: string; data?: unknown };

export type RpcRequest = { v: typeof PROTOCOL_VERSION; kind: 'req'; id: number; method: string; params?: unknown };

export type RpcResponse =
  | { v: typeof PROTOCOL_VERSION; kind: 'res'; id: number; ok: true; result?: unknown }
  | { v: typeof PROTOCOL_VERSION; kind: 'res'; id: number; ok: false; error: RpcErrorPayload };

export type RpcEvent = { v: typeof PROTOCOL_VERSION; kind: 'evt'; topic: string; data?: unknown };

export type RpcMessage = RpcRequest | RpcResponse | RpcEvent;

/** What a call, a handler or the channel failed with: a code a caller can branch on, and the remote's message. */
export class RpcError extends Error {
  readonly code: RpcErrorCode;
  readonly data?: unknown;

  constructor(code: RpcErrorCode, message: string, data?: unknown) {
    super(message);
    this.name = 'RpcError';
    this.code = code;
    this.data = data;
  }
}

export function encode(message: RpcMessage): string {
  return JSON.stringify(message);
}

export type Decoded =
  | { type: 'message'; message: RpcMessage }
  /** Anything that is not this protocol (another channel's messages share the WebView's). */
  | { type: 'foreign' }
  /** One of ours, spoken in a version this side does not know. */
  | { type: 'version'; version: unknown };

const KINDS = new Set(['req', 'res', 'evt']);

export function decode(raw: string): Decoded {
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return { type: 'foreign' };
  }
  if (typeof value !== 'object' || value === null) return { type: 'foreign' };
  const m = value as Record<string, unknown>;
  if (typeof m.kind !== 'string' || !KINDS.has(m.kind) || !('v' in m)) return { type: 'foreign' };
  if (m.v !== PROTOCOL_VERSION) return { type: 'version', version: m.v };
  return { type: 'message', message: value as RpcMessage };
}
