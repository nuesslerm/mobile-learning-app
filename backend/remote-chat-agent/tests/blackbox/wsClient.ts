/**
 * A WebSocket client for the black-box tests: every frame the service sends is kept in order, and `next` waits for the first
 * unread one that matches. Text and binary frames are both read as UTF-8 text, as the service reads them.
 */
import { WebSocket, type RawData } from 'ws';

import { API_SECRET, type Service } from './harness.js';

export type Frame = Record<string, unknown> & { kind?: string };

export class WsClient {
  readonly frames: Frame[] = [];
  readonly pings: number[] = [];
  closed: { code: number; reason: string } | null = null;
  private unread: Frame[] = [];
  private waiters: Array<() => void> = [];

  private constructor(readonly socket: WebSocket) {
    socket.on('message', (data: RawData) => {
      const frame = JSON.parse(Buffer.from(data as Buffer).toString('utf8')) as Frame;
      this.frames.push(frame);
      this.unread.push(frame);
      this.wake();
    });
    socket.on('ping', () => this.pings.push(Date.now()));
    socket.on('close', (code, reason) => {
      this.closed = { code, reason: reason.toString('utf8') };
      this.wake();
    });
    socket.on('error', () => undefined);
  }

  /** Opens `/ws` with the service's secret as the token; resolves once the socket is open. */
  static async open(
    svc: Service,
    opts: { token?: string | null; path?: string; protocols?: string[] } = {}
  ): Promise<WsClient> {
    const token = opts.token === undefined ? API_SECRET : opts.token;
    const url = `${svc.base.replace('http', 'ws')}${opts.path ?? '/ws'}${token === null ? '' : `?token=${encodeURIComponent(token)}`}`;
    const socket = new WebSocket(url, opts.protocols);
    const client = new WsClient(socket);
    await new Promise<void>((resolve, reject) => {
      socket.once('open', () => resolve());
      socket.once('error', reject);
      socket.once('unexpected-response', (_req, res) => reject(new Error(`upgrade refused: ${res.statusCode}`)));
    });
    return client;
  }

  /** What an upgrade that is refused answers: its status and body. */
  static async refused(svc: Service, token: string | null): Promise<{ status: number; body: string }> {
    const url = `${svc.base.replace('http', 'ws')}/ws${token === null ? '' : `?token=${encodeURIComponent(token)}`}`;
    const socket = new WebSocket(url);
    return new Promise((resolve, reject) => {
      socket.once('open', () => reject(new Error('upgrade was accepted')));
      socket.once('error', () => undefined);
      socket.once('unexpected-response', (_req, res) => {
        let body = '';
        res.on('data', (c) => (body += String(c)));
        res.on('end', () => resolve({ status: res.statusCode ?? 0, body }));
      });
    });
  }

  private wake(): void {
    const waiting = this.waiters;
    this.waiters = [];
    for (const w of waiting) w();
  }

  send(message: unknown): void {
    this.socket.send(typeof message === 'string' ? message : JSON.stringify(message));
  }

  sendBinary(message: unknown): void {
    this.socket.send(Buffer.from(JSON.stringify(message)), { binary: true });
  }

  /** The first frame not yet read that satisfies `match`; frames before it that do not are skipped over and stay in `frames`. */
  async next(match: (frame: Frame) => boolean = () => true, timeoutMs = 5000): Promise<Frame> {
    const deadline = Date.now() + timeoutMs;
    for (;;) {
      const at = this.unread.findIndex(match);
      if (at >= 0) return this.unread.splice(at, 1)[0] as Frame;
      if (this.closed) throw new Error(`socket closed (${this.closed.code}) while waiting for a frame`);
      const left = deadline - Date.now();
      if (left <= 0) throw new Error(`no matching frame in ${timeoutMs} ms; got ${JSON.stringify(this.frames)}`);
      await new Promise<void>((resolve) => {
        const timer = setTimeout(resolve, left);
        this.waiters.push(() => {
          clearTimeout(timer);
          resolve();
        });
      });
    }
  }

  /** Drops every frame not yet read, so a later `quiet` sees only what arrives after this. */
  flush(): void {
    this.unread = [];
  }

  /** The frames that arrive within `ms`, for a test that asserts something did not happen. */
  async quiet(ms = 300): Promise<Frame[]> {
    await new Promise((r) => setTimeout(r, ms));
    return this.unread.splice(0);
  }

  /** Sends a request and returns its response. */
  async request(id: string, method: string, params?: unknown): Promise<Frame> {
    this.send({ kind: 'request', id, method, params: params === undefined ? {} : params });
    return this.next((f) => f.kind === 'response' && f.id === id);
  }

  async waitClosed(timeoutMs = 5000): Promise<{ code: number; reason: string }> {
    const deadline = Date.now() + timeoutMs;
    while (!this.closed) {
      if (Date.now() > deadline) throw new Error('socket did not close');
      await new Promise<void>((resolve) => {
        const timer = setTimeout(resolve, 50);
        this.waiters.push(() => {
          clearTimeout(timer);
          resolve();
        });
      });
    }
    return this.closed;
  }

  close(): void {
    this.socket.terminate();
  }
}
