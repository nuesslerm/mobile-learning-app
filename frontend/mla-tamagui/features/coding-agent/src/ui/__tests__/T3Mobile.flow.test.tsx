import {
  fireEvent,
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
  providerInstanceId: 'opencode',
  runtimeMode: 'approval-required' as const,
  id: 'thread-42',
  projectId: 'project',
  title: 'Review the changed files',
};

function backend(openTab: (id: string) => Promise<void> = async () => {}) {
  let messages: T3Snapshot['messages'] = [];
  let preferences: Awaited<ReturnType<T3MobilePort['workspacePreferences']>> = {
    groupBy:'repo', sortBy:'recent', projectOrderBy:'manual', cardLayout:'detailed',
    worktreeCardProperties:['pr','ports'],agentActivityDisplayMode:'compact',
    filters:{hideSleepingWorkspaces:false,alwaysShowDefaultBranchWorkspace:true,hideDefaultBranchWorkspace:false,hideAutomationGeneratedWorkspaces:false,
      hideCliCreatedWorkspaces:false,hideDetachedHeadWorkspaces:false,hideWorkspacesFromOtherDevices:false,filterRepoIds:[],visibleHostIds:null},
  };
  const port: T3MobilePort = {
    workspacePreferences: async () => preferences,
    saveWorkspacePreferences: async (_pair,patch) => {
      preferences={...preferences,...patch,filters:{...preferences.filters,...patch.filters}};
      return preferences;
    },
    restore: async () => pair,
    sessionContext: async () => ({
      workspace: 'Mobile checkout',
      sessions: [thread, {id: 'thread-43', title: 'Fix the parser', needsProjection: true}],
    }),
    pair: async () => pair,
    workspaces: async () => ({host:{name:'Fixture Mac',os:'darwin'}, repo:'Fixture repository',
      checkouts:[{id:'repo::/fixture', name:'Mobile checkout', branch:null, main:false}]}),
    editWorkspace: async (_pair, id, edit) => ({id, name: 'displayName' in edit ? edit.displayName || 'Mobile checkout' : 'Mobile checkout', branch:null,main:false,
      automaticName:'Mobile checkout', pinned:'isPinned' in edit && edit.isPinned}),
    checkoutSessions: async () => ({active:thread.id, sessions:[thread]}),
    shell: async () => ({
      projects: [{ id: 'project', title: 'Fixture repository' }],
      threads: [thread],
    }),
    open: async (_pair, _id, update) => {
      update({ thread, messages });
      return {
        models: async () => [{ slug: 'default', name: 'Default' }],
        async setModel() {},
        async setRuntimeMode() {},
        openTab,
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
    expect(await screen.findByRole('button',{name:'Open Fixture Mac'})).toBeTruthy();
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

  it('Given an opened attachment, when a reader copies the image and retries a failed save, then the viewer stays on that image', async () => {
    const port = backend();
    const open = port.open;
    const attachment = {id: 'blue-orange', name: 'swatch.png', mimeType: 'image/png', type: 'image' as const};
    const uri = 'https://fixture.invalid/assets/swatch';
    port.open = async (pairing, id, update, disconnected) => {
      const session = await open(pairing, id, () => update({
        thread, messages: [{id: 'photo', role: 'user', text: 'Swatch', attachments: [attachment]}],
      }), disconnected);
      return {...session, resolveImage: async () => ({uri, expiresAt: Date.now() + 60000})};
    };
    const copied: string[] = [];
    let saves = 0;
    await app.render('/coding-agent/thread-42', {overrides: {'coding-agent': {
      t3Mobile: port,
      imageActionsFor: (image: {uri: string}) => ({
        copy: async () => { copied.push(image.uri); },
        save: async () => { if (++saves === 1) throw new Error('Photos access denied'); },
      }),
    }}});
    await userEvent.click(await screen.findByLabelText('swatch.png'));
    await userEvent.click(await screen.findByRole('button', {name:'Image actions'}));
    expect(screen.queryByRole('button', {name:'Share…'})).toBeNull();
    await userEvent.click(screen.getByRole('button', {name:'Copy image'}));
    expect(await screen.findByText('Image copied')).toBeTruthy();
    expect(copied).toEqual([uri]);
    await userEvent.click(screen.getByRole('button', {name:'Image actions'}));
    await userEvent.click(screen.getByRole('button', {name:'Save image'}));
    expect(await screen.findByText("Couldn't save image")).toBeTruthy();
    await userEvent.click(screen.getByRole('button', {name:'Image actions'}));
    await userEvent.click(screen.getByRole('button', {name:'Save image'}));
    expect(await screen.findByText('Image saved')).toBeTruthy();
  });

  it('Given a paired workspace, when the reader changes card layout, then reopening options restores it', async () => {
    const port = backend();
    await app.render('/coding-agent', {search:{host:'1'}, overrides: { 'coding-agent': { t3Mobile: port } } });
    await userEvent.click(await screen.findByRole('button', {name:'Workspace options'}));
    await userEvent.click(await screen.findByRole('button', {name:'Card layout, Detailed'}));
    await userEvent.click(await screen.findByRole('radio', {name:'Compact'}));
    await waitFor(async () => expect((await port.workspacePreferences(pair,new AbortController().signal)).cardLayout).toBe('compact'));
    await userEvent.click(screen.getByRole('button',{name:'Done'}));
    await userEvent.click(screen.getByRole('button',{name:'Workspace options'}));
    await userEvent.click(await screen.findByRole('button',{name:'Card layout, Compact'}));
    expect(await screen.findByRole('radio',{name:'Compact',checked:true})).toBeTruthy();
  });

  it('Given a checkout, when the reader renames it from its context menu, then the host list shows the saved label', async () => {
    const port = backend();
    await app.render('/coding-agent', {search:{host:'1'}, overrides:{'coding-agent':{t3Mobile:port}}});
    const checkout = await screen.findByRole('button', {name:'Mobile checkout'});
    await userEvent.pointer({keys:'[MouseRight]', target:checkout});
    const backdrop = await screen.findByRole('button', {name:'Dismiss'});
    fireEvent.click(backdrop, { detail: 1 });
    await userEvent.click(screen.getByRole('button',{name:'Rename'}));
    const field = screen.getByRole('textbox',{name:'Workspace name'});
    await userEvent.clear(field);
    await userEvent.type(field,'Saved label');
    await userEvent.click(screen.getByRole('button',{name:'Save'}));
    expect(await screen.findByRole('button',{name:'Saved label'})).toBeTruthy();
    expect(screen.queryByRole('button',{name:'Mobile checkout'})).toBeNull();
  });

  it('Given a connected chat, when the reader selects a model, then the current thread receives that model', async () => {
    const port = backend();
    const connect = port.open;
    const selections: string[][] = [];
    port.open = async (...args) => ({
      ...await connect(...args),
      models: async () => [{ slug: 'minimax/flash', name: 'MiniMax Flash' }],
      setModel: async (id, provider, model) => { selections.push([id, provider, model]); },
    });
    await app.render('/coding-agent/thread-42', {overrides: {'coding-agent': {t3Mobile: port}}});
    await userEvent.click(await screen.findByRole('button', {name: 'Model, Model'}));
    await userEvent.click(await screen.findByRole('button', {name: 'MiniMax Flash'}));
    expect(selections).toEqual([['new-message', 'opencode', 'minimax/flash']]);
    await userEvent.click(screen.getByRole('button', {name: 'Done'}));
    expect(screen.queryByRole('button', {name: 'MiniMax Flash'})).toBeNull();
  });

  it('Given two chats in the workspace, when a reader picks its other tab, then that thread opens through the Orca T3 route', async () => {
    const opened: string[] = [];
    const port = backend(async (id) => { opened.push(id); });
    await app.render('/coding-agent/thread-42', {
      overrides: {'coding-agent': {t3Mobile: port}},
    });
    expect(await screen.findByText('Mobile checkout')).toBeTruthy();
    await userEvent.click(screen.getByRole('button', {name: '2 tabs'}));
    await userEvent.click(await screen.findByRole('button', {name: 'Fix the parser'}));
    expect(opened).toEqual(['thread-43']);
    await waitFor(() => expect(routerHarness().calls()).toContainEqual(['replace', '/coding-agent/thread-43']));
  });

  it('Given a paired desktop, when opening its checkout and sending in the saved chat, then the reply and cleared composer are visible', async () => {
    const port = backend();
    await app.render('/coding-agent', {
      overrides: { 'coding-agent': { t3Mobile: port } },
    });
    await userEvent.click(await screen.findByRole('button',{name:'Open Fixture Mac'}));
    expect(routerHarness().lastPush()).toBe('/coding-agent?host=1');
    await app.render('/coding-agent', {search:{host:'1'},overrides:{'coding-agent':{t3Mobile:port}}});
    await userEvent.click(await screen.findByRole('button',{name:'Mobile checkout'}));
    await waitFor(() =>
      expect(routerHarness().lastPush()).toBe('/coding-agent/thread-42?checkout=Mobile%20checkout')
    );
    await app.render('/coding-agent/thread-42', {
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
