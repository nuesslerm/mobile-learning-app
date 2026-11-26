import { describe, expect, it, vi } from 'vitest';
import { openConversation } from '../session';

describe('T3 subscription and image requests', () => {
  it.each([
    [[{id: 'other', value: true}, {id: 'depth', value: 'low'}], [{id: 'other', value: true}, {id: 'depth', value: 'high'}]],
    [{other: true, depth: 'low'}, {other: true, depth: 'high'}],
  ])('Given a saved model selection, when changing its advertised effort, then the dispatch preserves other options', async (options, expected) => {
    const outgoing = vi.fn();
    const ws = {send: (raw: string) => outgoing(JSON.parse(raw)), close: vi.fn()} as unknown as WebSocket;
    const chat = openConversation(ws, 'https://fixture.test', 'thread', vi.fn(), vi.fn());
    function receive(frame: unknown) {
      ws.onmessage?.call(ws, {data: JSON.stringify(frame)} as MessageEvent);
    }
    receive({_tag: 'Chunk', requestId: '1', values: [{ kind: 'snapshot', snapshotSequence: 1, projection: {
      thread: {id: 'thread', projectId: 'repo', title: 'Review', modelSelection: {instanceId: 'driver', model: 'm', options}},
      messages: [],
    } }]});
    const models = chat.models('driver');
    const request = outgoing.mock.lastCall?.[0];
    expect(request.tag).toBe('server.getConfig');
    receive({_tag: 'Exit', requestId: request.id, exit: {_tag: 'Success', value: {providers: [{instanceId: 'driver', models: [
      {slug: 'm', name: 'Example', capabilities: {optionDescriptors: [
        {type: 'boolean', id: 'other'}, {type: 'select', id: 'depth', role: 'reasoning', options: [{id: 'high', label: 'High'}]},
      ]}},
      {slug: 'basic', name: 'Basic', capabilities: null},
    ]}]}}});
    expect(await models).toEqual([
      {slug: 'm', name: 'Example', effort: {id: 'depth', current: 'low', choices: [{slug: 'high', name: 'High'}]}},
      {slug: 'basic', name: 'Basic'},
    ]);
    const save = chat.setModel('change', 'driver', 'm', {id: 'depth', value: 'high'});
    const dispatch = outgoing.mock.lastCall?.[0];
    expect(dispatch.payload).toEqual({type: 'thread.model-selection.set', commandId: 'change', threadId: 'thread',
      modelSelection: {instanceId: 'driver', model: 'm', options: expected}});
    receive({_tag: 'Exit', requestId: dispatch.id, exit: {_tag: 'Success', value: {}}});
    await save;
    await expect(chat.setModel('stale', 'driver', 'old', {id: 'depth', value: 'high'})).rejects.toThrow('model changed');
    expect(outgoing.mock.lastCall?.[0]).toBe(dispatch);
    chat.close();
  });

  it('Given split history and two images, when responses arrive in reverse order, then each image gets its own signed URL', async () => {
    const outbound: { id: string; tag: string; payload: unknown }[] = [];
    const ws = {
      send: (raw: string) => outbound.push(JSON.parse(raw)),
      close: vi.fn(),
      onmessage: null,
    } as unknown as WebSocket;
    const update = vi.fn(),
      disconnect = vi.fn();
    const chat = openConversation(
      ws,
      'https://fixture.test',
      'thread',
      update,
      disconnect
    );
    const receive = (data: string) =>
      ws.onmessage?.call(ws, { data } as MessageEvent);
    const attachments = ['left', 'right'].map((id) => ({
      id,
      type: 'image' as const,
      name: id + '.png',
      mimeType: 'image/png',
    }));
    const snapshot = JSON.stringify({
      _tag: 'Chunk',
      requestId: '1',
      values: [
        {
          kind: 'snapshot',
          snapshotSequence: 10,
          projection: {
            thread: { id: 'thread', projectId: 'repo', title: 'Photos' },
            messages: [{ id: 'm1', role: 'user', text: 'Both', attachments }],
          },
        },
      ],
    });
    receive('\u001e' + snapshot.slice(0, 17));
    expect(update).not.toHaveBeenCalled();
    receive(snapshot.slice(17));
    expect(update.mock.calls[0][0].messages[0].attachments).toEqual(
      attachments
    );
    const images = attachments.map((a) => chat.resolveImage(a));
    const requests = outbound.filter((r) => r.tag === 'assets.createUrl');
    for (const [i, request] of requests.entries())
      expect(request.payload).toEqual({
        resource: {
          _tag: 'attachment',
          attachmentId: attachments[i].id,
          fileName: attachments[i].name,
          mimeType: 'image/png',
          disposition: 'inline',
        },
      });
    for (const i of [1, 0])
      receive(
        JSON.stringify({
          _tag: 'Exit',
          requestId: requests[i].id,
          exit: {
            _tag: 'Success',
            value: {
              relativeUrl: '/api/assets/' + i,
              expiresAt: 50000,
            },
          },
        })
      );
    expect((await Promise.all(images)).map((x) => x.uri)).toEqual([
      'https://fixture.test/api/assets/0',
      'https://fixture.test/api/assets/1',
    ]);
    chat.close();
    expect(disconnect).not.toHaveBeenCalled();
  });
});
