import {
  routerHarness,
  screen,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';
import { app } from '../../__tests__/flowApp';
import type { T3MobilePort, T3Snapshot } from '../../domain/t3Mobile';

const pair = { endpoint: 'https://fixture.invalid', token: 'fixture-token' };
const thread = {
  id: 'thread-42',
  projectId: 'project',
  title: 'Review the changed files',
};

function backend() {
  let messages: T3Snapshot['messages'] = [];
  const port: T3MobilePort = {
    restore: async () => pair,
    pair: async () => pair,
    shell: async () => ({
      projects: [{ id: 'project', title: 'Fixture repository' }],
      threads: [thread],
    }),
    open: async (_pair, _id, update) => {
      update({ thread, messages });
      return {
        close() {},
        async resolveImage() {
          throw new Error('No fixture image');
        },
        async send(id, text) {
          messages = [
            ...messages,
            { id, role: 'user', text },
            { id: 'answer', role: 'assistant', text: 'Ready to review.' },
          ];
          update({ thread, messages });
        },
      };
    },
    newId: () => 'new-message',
  };
  return port;
}

describe('Orca chat with the T3 transport', () => {
  it('Given an unpaired desktop, when retrying a bad invite and pasting the T3 link, then only the valid grant is exchanged and its project appears', async () => {
    const port = backend();
    port.restore = async () => null;
    const exchanges: string[][] = [];
    port.pair = async (endpoint, code) => {
      exchanges.push([endpoint, code]);
      return pair;
    };
    const camera = { get: async () => ({granted: false, canAskAgain: true}), request: async () => ({granted: true, canAskAgain: true}) };
    await app.render('/coding-agent', {
      search: { presentation: 'orca', transport: 't3' },
      overrides: { 'coding-agent': { t3Mobile: port, pairingCamera: camera } },
    });
    await userEvent.click(await screen.findByRole('button', { name: 'Pair desktop' }));
    await userEvent.click(await screen.findByRole('button', { name: 'Paste code instead' }));
    await userEvent.type(screen.getByTestId('MobilePairCode'), 'bad-invite');
    await userEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText(/Not a valid pairing code/)).toBeTruthy();
    expect(exchanges).toEqual([]);
    await userEvent.click(screen.getByRole('button', { name: 'Try Again' }));
    await userEvent.click(screen.getByRole('button', { name: 'Paste code instead' }));
    await userEvent.type(screen.getByTestId('MobilePairCode'), 'https://fixture.invalid/pair#token=fixture-code');
    await userEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText(thread.title)).toBeTruthy();
    expect(exchanges).toEqual([['https://fixture.invalid', 'fixture-code']]);
  });

  it('Given a reply with a tool between passages, when the reader copies the progress update, then only that passage reaches the clipboard', async () => {
    const port = backend();
    const open = port.open;
    const progress = 'Reading the file.';
    port.open = (pairing, id, update, disconnected) =>
      open(pairing, id, () => update({
        thread,
        messages: [{
          id: 'answer', role: 'assistant', text: progress + 'Review complete.', runId: 'run',
        }],
        turnItems: [{id: 'read', type: 'command_execution', name: 'read', runId: 'run'}],
        toolTextOffsets: {read: progress.length},
      }), disconnected);
    const copied: string[] = [];
    await app.render('/coding-agent/thread-42', {
      search: { presentation: 'orca', transport: 't3' },
      overrides: { 'coding-agent': {
        t3Mobile: port,
        copyMessage: async (text: string) => { copied.push(text); return true; },
      } },
    });
    const copies = await screen.findAllByRole('button', {name: 'Copy message'});
    expect(copies).toHaveLength(2);
    const tool = screen.getByRole('button', {name: 'Read 1 file'});
    expect(tool.compareDocumentPosition(copies[0]) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    await userEvent.click(copies[0]);
    expect(await screen.findByRole('button', {name:'Copied'})).toBeTruthy();
    expect(copied).toEqual([progress]);
  });

  it('Given a paired project, when opening a thread from its shell and sending, then the reply and cleared composer are visible', async () => {
    const port = backend();
    const home = await app.render('/coding-agent', {
      search: { presentation: 'orca', transport: 't3' },
      overrides: { 'coding-agent': { t3Mobile: port } },
    });
    await screen.findByText(thread.title);
    await home.tap('MobileThread-' + thread.id);
    await waitFor(() =>
      expect(routerHarness().lastPush()).toContain(thread.id)
    );
    await app.render('/coding-agent/thread-42', {
      search: { presentation: 'orca', transport: 't3' },
      overrides: { 'coding-agent': { t3Mobile: port } },
    });
    await screen.findByRole('button', { name: 'Send message' });
    const field = screen.getByPlaceholderText(
      'Ask anything, @ to mention files, / for commands'
    );
    await userEvent.type(field, 'List the changes.');
    await userEvent.click(screen.getByRole('button', { name: 'Send message' }));
    expect(await screen.findByText('Ready to review.')).toBeTruthy();
    expect((field as HTMLTextAreaElement).value).toBe('');
  });
});
