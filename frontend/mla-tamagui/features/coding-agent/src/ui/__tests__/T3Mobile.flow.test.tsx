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
