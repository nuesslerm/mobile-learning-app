// A subscription made before the socket opens reaches the server once: the open handler both resubscribes and flushes the queue.
import { describe, expect, it } from 'vitest';

import { createWsGateway } from '../data/wsGateway';

type Listener = (event: unknown) => void;

class FakeSocket {
  static OPEN = 1;
  static instances: FakeSocket[] = [];

  readyState = 0;
  readonly sent: Array<Record<string, unknown>> = [];
  private readonly listeners = new Map<string, Listener[]>();

  constructor(readonly url: string) {
    FakeSocket.instances.push(this);
  }

  addEventListener(type: string, listener: Listener): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }
  send(data: string): void {
    this.sent.push(JSON.parse(data) as Record<string, unknown>);
  }
  close(): void {
    this.readyState = 3;
  }
  open(): void {
    this.readyState = 1;
    for (const listener of this.listeners.get('open') ?? []) listener({});
  }
}

describe('subscriptions made before the socket opens', () => {
  it('given a shell and a thread subscription made while the socket is closed, when it opens, then the server is sent each subscribe exactly once', () => {
    // Given
    FakeSocket.instances = [];
    const gateway = createWsGateway({
      wsUrl: 'ws://agent.test:8787/ws',
      token: 't',
      WebSocketImpl: FakeSocket as unknown as typeof WebSocket,
    });
    gateway.subscribeShell(() => undefined);
    gateway.subscribeThread('thread-1', () => undefined, 7);
    const socket = FakeSocket.instances[0] as FakeSocket;

    // When
    socket.open();

    // Then
    const subscribes = socket.sent.filter((frame) => frame.kind === 'subscribe');
    expect(subscribes.map((frame) => frame.method).sort()).toEqual([
      'orchestration.subscribeShell',
      'orchestration.subscribeThread',
    ]);
    expect(new Set(subscribes.map((frame) => frame.id)).size).toBe(2);
    gateway.close();
  });
});
