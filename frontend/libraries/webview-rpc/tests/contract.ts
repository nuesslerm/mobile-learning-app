import { describe, expect, it, vi } from 'vitest';

import { PROTOCOL_VERSION, RpcError, createRpcClient, createRpcServer, encode, type RpcTransport } from '../src';

export type Pair = { host: RpcTransport; page: RpcTransport };

const settled = () => new Promise((resolve) => setTimeout(resolve, 5));

/** What every transport pair must do: the host calls, the page answers and emits. */
export function rpcContract(name: string, makePair: () => Pair): void {
  describe(`the rpc protocol over ${name}`, () => {
    it('given a handler on the page, when the host calls it, then the result comes back', async () => {
      // Given a page that answers `add`
      const { host, page } = makePair();
      createRpcServer(page, { add: (p) => (p as { a: number; b: number }).a + (p as { a: number; b: number }).b });
      const client = createRpcClient(host);

      // When the host calls it
      const result = await client.call<number>('add', { a: 2, b: 3 });

      // Then the answer is the sum
      expect(result).toBe(5);
    });

    it('given a handler that throws, when the host calls it, then the call fails with a remote error carrying the message', async () => {
      // Given a page whose handler throws
      const { host, page } = makePair();
      createRpcServer(page, { boom: () => { throw new Error('it broke'); } });
      const client = createRpcClient(host);

      // When the host calls it
      const failure = await client.call('boom').catch((e: unknown) => e);

      // Then the error is a remote RpcError with the handler's message
      expect(failure).toBeInstanceOf(RpcError);
      expect(failure).toMatchObject({ code: 'remote', message: 'it broke' });
    });

    it('given a handler whose result JSON cannot encode, when the host calls it, then the call fails at once as a remote error and does not wait for its timeout', async () => {
      // Given a page whose handler returns a value with no JSON form, and a host that would wait a minute
      const { host, page } = makePair();
      createRpcServer(page, { big: () => 10n });
      const client = createRpcClient(host, { timeoutMs: 60_000 });

      // When the host calls it
      const failure = await client.call('big').catch((e: unknown) => e);

      // Then the error is a remote RpcError that says the answer could not be encoded
      expect(failure).toBeInstanceOf(RpcError);
      expect(failure).toMatchObject({ code: 'remote' });
      expect((failure as RpcError).message).toContain('could not be encoded');
    });

    it('given a method nobody handles, when the host calls it, then the call fails as an unknown method', async () => {
      // Given a page with no handler for the method
      const { host, page } = makePair();
      createRpcServer(page, {});
      const client = createRpcClient(host);

      // When the host calls it
      const failure = await client.call('nope').catch((e: unknown) => e);

      // Then the error says the method is unknown
      expect(failure).toMatchObject({ code: 'unknown-method' });
    });

    it('given a handler that never answers, when the host waits, then the call fails with a timeout', async () => {
      // Given a page whose handler never settles
      const { host, page } = makePair();
      createRpcServer(page, { hang: () => new Promise(() => undefined) });
      const client = createRpcClient(host, { timeoutMs: 20 });

      // When the host calls it
      const failure = await client.call('hang').catch((e: unknown) => e);

      // Then the call times out
      expect(failure).toMatchObject({ code: 'timeout' });
    });

    it('given two calls answered out of order, when both are in flight, then each gets its own answer', async () => {
      // Given a page that answers the slow call after the quick one
      const { host, page } = makePair();
      createRpcServer(page, {
        echo: (p) => new Promise((resolve) => setTimeout(() => resolve(p), (p as { ms: number }).ms)),
      });
      const client = createRpcClient(host);

      // When the slow call is sent first and the quick one second
      const [slow, quick] = await Promise.all([client.call('echo', { ms: 30, tag: 'slow' }), client.call('echo', { ms: 1, tag: 'quick' })]);

      // Then each call resolves with its own params
      expect(slow).toEqual({ ms: 30, tag: 'slow' });
      expect(quick).toEqual({ ms: 1, tag: 'quick' });
    });

    it('given a subscription, when the page emits on its topic, then the listener hears it until it unsubscribes', async () => {
      // Given a host subscribed to `tick`
      const { host, page } = makePair();
      const server = createRpcServer(page, {});
      const client = createRpcClient(host);
      const heard = vi.fn();
      const stop = client.subscribe('tick', heard);

      // When the page emits twice, and the host unsubscribes between them
      server.emit('tick', 1);
      await settled();
      stop();
      server.emit('tick', 2);
      await settled();

      // Then only the first event was heard
      expect(heard.mock.calls).toEqual([[1]]);
    });

    it('given calls in flight, when the client closes, then they fail as closed and later calls fail too', async () => {
      // Given a call that is still waiting
      const { host, page } = makePair();
      createRpcServer(page, { hang: () => new Promise(() => undefined) });
      const client = createRpcClient(host);
      const waiting = client.call('hang').catch((e: unknown) => e);

      // When the client closes
      client.close();

      // Then the waiting call fails as closed, and so does a new one
      expect(await waiting).toMatchObject({ code: 'closed' });
      expect(await client.call('later').catch((e: unknown) => e)).toMatchObject({ code: 'closed' });
    });

    it('given messages that are not this protocol, when they arrive, then both sides ignore them and carry on', async () => {
      // Given a working pair, and a stray message of another channel sent to each side
      const { host, page } = makePair();
      createRpcServer(page, { ok: () => 'fine' });
      const client = createRpcClient(host);
      host.send(JSON.stringify({ type: 'focus' }));
      page.send('not json at all');

      // When a real call follows
      const result = await client.call('ok');

      // Then it still works
      expect(result).toBe('fine');
    });

    it('given a message in another protocol version, when it arrives, then the client reports a protocol error', async () => {
      // Given a client that reports protocol errors
      const { host, page } = makePair();
      const onProtocolError = vi.fn();
      createRpcClient(host, { onProtocolError });

      // When the page sends a response in a version the client does not know
      page.send(encode({ v: (PROTOCOL_VERSION + 1) as typeof PROTOCOL_VERSION, kind: 'res', id: 1, ok: true }));
      await settled();

      // Then the client reported one protocol error naming the versions
      expect(onProtocolError).toHaveBeenCalledOnce();
      expect(onProtocolError.mock.calls[0]?.[0]).toMatchObject({ code: 'protocol' });
    });
  });
}
