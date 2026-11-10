// features/coding-agent/src/__tests__/wsLogging.flow.test.ts
//
// What a phone session leaves in BetterStack when a thread screen stays blank: which thread was subscribed, that the
// server refused it and why, and how the socket then ended. The socket is the WebSocket constructor, doubled.
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { createWsGateway } from '../data/wsGateway';
import { captureLogs } from './captureLogs';

const TOKEN = 'bearer-token-for-the-test';

type Listener = (event: unknown) => void;

class FakeSocket {
  static OPEN = 1;
  static instances: FakeSocket[] = [];
  static latest(): FakeSocket {
    return FakeSocket.instances[FakeSocket.instances.length - 1] as FakeSocket;
  }

  readyState = 0;
  private readonly listeners = new Map<string, Listener[]>();

  constructor(readonly url: string) {
    FakeSocket.instances.push(this);
  }

  addEventListener(type: string, listener: Listener): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }
  send(): void {}
  close(): void {
    this.readyState = 3;
  }

  open(): void {
    this.readyState = 1;
    this.emit('open', {});
  }
  drop(close: { code: number; reason: string; wasClean: boolean }): void {
    this.readyState = 3;
    this.emit('close', close);
  }
  receive(frame: unknown): void {
    this.emit('message', { data: JSON.stringify(frame) });
  }
  private emit(type: string, event: unknown): void {
    for (const listener of this.listeners.get(type) ?? []) listener(event);
  }
}

describe('thread screen over a socket whose server refuses the subscription', () => {
  let logs: ReturnType<typeof captureLogs>;

  beforeEach(() => {
    logs = captureLogs();
    FakeSocket.instances = [];
  });

  afterEach(() => logs.stop());

  it('given a thread subscription, when the socket opens, the server refuses the subscription and the socket drops, then the lines name the thread, the refusal code and how the socket ended, and none carries the token', () => {
    // Given a gateway whose socket URL carries the bearer token, and a screen subscribed to a thread
    const gateway = createWsGateway({
      wsUrl: 'ws://agent.test:8787/ws',
      token: TOKEN,
      WebSocketImpl: FakeSocket as unknown as typeof WebSocket,
    });
    gateway.subscribeThread('thread-1', () => undefined, 7);
    const socket = FakeSocket.latest();
    expect(socket.url).toContain(`token=${TOKEN}`);

    // When the socket opens, the server answers the subscription id with a refusal, and the socket then drops
    socket.open();
    socket.receive({
      kind: 'response',
      id: 's1',
      ok: false,
      error: { code: 'subscription_failed', message: 'thread not found' },
    });
    socket.drop({ code: 1006, reason: 'going away', wasClean: false });

    // Then the subscribe line and the refusal name the same thread, and the refusal says why
    const subscribe = logs.named('coding-agent.stream.subscribe')[0];
    expect(subscribe?.fields).toMatchObject({
      method: 'orchestration.subscribeThread',
      threadId: 'thread-1',
      afterSequence: 7,
      resumed: false,
    });
    const refused = logs.named('coding-agent.stream.subscribe_failed');
    expect(refused).toHaveLength(1);
    expect(refused[0]).toMatchObject({
      level: 'warn',
      fields: {
        clientId: subscribe?.fields.clientId,
        method: 'orchestration.subscribeThread',
        threadId: 'thread-1',
        code: 'subscription_failed',
        message: 'thread not found',
      },
    });

    // And the socket's own lines say where it connected and how it ended
    expect(logs.named('coding-agent.ws.open')[0]?.fields).toMatchObject({
      host: 'agent.test:8787',
      attempt: 1,
      subscriptions: 1,
    });
    expect(logs.named('coding-agent.ws.close')[0]?.fields).toMatchObject({
      host: 'agent.test:8787',
      code: 1006,
      wasClean: false,
      reason: 'going away',
    });
    expect(logs.dump()).not.toContain(TOKEN);
    expect(logs.dump()).not.toContain('token=');
    gateway.close();
  });
});
