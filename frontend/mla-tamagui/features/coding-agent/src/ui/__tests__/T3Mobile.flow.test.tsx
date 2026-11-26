import {
  fireEvent,
  routerHarness,
  screen,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';
import { describe, expect, it, vi } from 'vitest';
import { app } from '../../__tests__/flowApp';
import { createSettingsPortDoubles } from '../../testing/settingsPorts';
import { createT3MobileApi } from '../../data/t3/mobileApi';
import { resumeCommand } from '../../domain/t3Mobile';
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
    home: async () => ({ host: { name: 'Fixture Mac', os: 'darwin' }, worktreeCount: 1 }),
    workspaces: async () => ({host:{name:'Fixture Mac',os:'darwin'}, repo:'Fixture repository',
      checkouts:[{id:'repo::/fixture', name:'Mobile checkout', branch:null, main:false}]}),
    editWorkspace: async (_pair, id, edit) => ({id, name: 'displayName' in edit ? edit.displayName || 'Mobile checkout' : 'Mobile checkout', branch:null,main:false,
      automaticName:'Mobile checkout', pinned:'isPinned' in edit && edit.isPinned, unread:'isUnread' in edit && edit.isUnread}),
    checkoutChanges: async () => ({branch:null,head:null,comparison:null}),
    checkoutFile: async () => ({kind:'text',original:null,modified:'created'}),
    checkoutCommit: async () => ({body:'', initial:true, files:[], limited:false}),
    checkoutHistory: async () => ({ head: null, branch: null, commits: [], nextOffset: null }),
    checkoutSessions: async () => ({active:thread.id, sessions:[thread]}),
    shell: async () => ({
      projects: [{ id: 'project', title: 'Fixture repository' }],
      threads: [thread],
    }),
    open: async (_pair, _id, update) => {
      update({ thread, messages });
      return {
        files: async () => [],
        listDirectory: async () => ({entries: [], truncated: false}),
        readFile: async () => ({contents: '', truncated: false}),
        catalog: async () => [],
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
  it('Given a paired chat with a draft, when text spacing changes in the header sheet, then it persists and the draft survives', async () => {
    const { preferences } = createSettingsPortDoubles();
    await app.render('/coding-agent/thread-42', {
      overrides: { 'coding-agent': { t3Mobile: backend(), preferences } },
    });
    const draft = await screen.findByPlaceholderText('Ask anything, @ to mention files, / for commands') as HTMLTextAreaElement;
    await userEvent.type(draft, 'Unsaved reply');
    await userEvent.click(screen.getByRole('button', {name: 'More actions'}));
    await userEvent.click(await screen.findByRole('button', {name: 'Text size'}));
    await userEvent.click(await screen.findByRole('button', {name: 'Compact'}));
    await waitFor(async () => expect((await preferences.readChatText())?.lineSpacing).toBe('compact'));
    await userEvent.click(screen.getByRole('button', {name:'Done'}));
    expect(draft.value).toBe('Unsaved reply');
    await userEvent.click(screen.getByRole('button', {name:'More actions'}));
    await userEvent.click(await screen.findByRole('button', {name:'Text size'}));
    await userEvent.click(await screen.findByRole('button', {name:'More text settings'}));
    expect(routerHarness().lastPush()).toBe('/coding-agent/native-chat-settings');
  });

  it('Given a paired workspace, when opening a nested file and returning, then the folder stays expanded and the chat draft survives', async () => {
    const port = backend();
    const open = port.open;
    const readFile = vi.fn().mockResolvedValue({contents:'export const answer = 42;', truncated:true});
    const listDirectory = vi.fn(async (path: string) => ({
      entries: path ? [{path:'src/main.ts', kind:'file' as const}] : [{path:'node_modules',kind:'directory' as const}, {path:'src',kind:'directory' as const}, {path:'.git',kind:'directory' as const}],
      truncated:false,
    }));
    port.open = async (...args) => ({...await open(...args), readFile, listDirectory});
    await app.render('/coding-agent/thread-42', {overrides: {'coding-agent': {t3Mobile: port}}});
    const draft = await screen.findByPlaceholderText('Ask anything, @ to mention files, / for commands') as HTMLTextAreaElement;
    await userEvent.type(draft,'Keep this draft');
    await userEvent.click(await screen.findByRole('button',{name:'More actions'}));
    await userEvent.click(await screen.findByRole('button',{name:'File explorer'}));
    const src = await screen.findByRole('button',{name:'src'});
    expect(screen.queryByRole('button',{name:'.git'})).toBeNull();
    expect(screen.queryByRole('button',{name:'node_modules'})).toBeNull();
    await userEvent.click(src);
    await userEvent.click(await screen.findByRole('button',{name:'main.ts'}));
    expect(await screen.findByText('export const answer = 42;')).toBeTruthy();
    expect(screen.getByText('Showing the first 1 MiB')).toBeTruthy();
    await userEvent.click(screen.getByRole('button',{name:'Back to files'}));
    expect(await screen.findByRole('button',{name:'main.ts'})).toBeTruthy();
    await userEvent.click(screen.getByRole('button',{name:'Back to chat'}));
    expect(draft.value).toBe('Keep this draft');
    expect(listDirectory.mock.calls).toEqual([[''],['src']]);
    expect(readFile.mock.calls).toEqual([['src/main.ts']]);
  });

  it('Given a paired host on an older server, when Feed gets a missing endpoint, then it uses the catalog and preserves authorization failures', async () => {
    const api = createT3MobileApi();
    const host = { name: 'Legacy desktop', os: 'linux' };
    const fetch = vi.fn<typeof globalThis.fetch>();
    vi.stubGlobal('fetch', fetch);
    try {
      fetch.mockResolvedValueOnce(new Response(null, { status: 404 }));
      fetch.mockResolvedValueOnce(Response.json({ host, repo: 'desk', checkouts: [] }));
      const signal = new AbortController().signal;
      expect(await api.home(pair, signal)).toEqual({ host, worktreeCount: 0 });
      expect(fetch.mock.calls.map(([url]) => url)).toEqual([
        pair.endpoint + '/api/mobile/home',
        pair.endpoint + '/api/mobile/workspaces',
      ]);
      fetch.mockClear();
      fetch.mockResolvedValueOnce(new Response(null, { status: 403 }));
      await expect(api.home(pair, signal)).rejects.toThrow('HTTP 403');
      expect(fetch).toHaveBeenCalledTimes(1);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('Given a paired RCA desktop without bots support, when Bots opens, then the notice names that desktop', async () => {
    await app.render('/coding-agent', { overrides: { 'coding-agent': { t3Mobile: backend() } } });
    await screen.findByRole('button', { name: 'Open Fixture Mac' });
    expect(screen.getByText('1 worktree · macOS')).toBeTruthy();
    await userEvent.click(screen.getByRole('tab', { name: 'Bots' }));
    expect(await screen.findByRole('heading', { name: 'Update Orca on Fixture Mac' })).toBeTruthy();
    expect(screen.getByText('Its version of Orca can’t show bots on your phone yet.')).toBeTruthy();
  });

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

  it('Given a detached checkout, when the reader opens Source Control and selects a commit, then they see its first-parent file changes and can navigate back', async () => {
    const port = backend();
    const sha = 'a'.repeat(40);
    const file = {path:'src/changed.ts', status:'modified', binary:false, additions:3, deletions:2};
    const commit = {sha, shortSha:sha.slice(0,7), subject:'Selected change', author:'Developer', authoredAtMs:10, parent:'b'.repeat(40)};
    const check = (id:string) => expect(id).toBe('repo::/fixture');
    port.checkoutChanges = async (_pair, id) => {
      check(id);
      return {head:sha, branch:null, comparison:{base:'main',baseOid:'c'.repeat(40),ahead:1,files:[file],limited:false}};
    };
    port.checkoutHistory = async (_pair, id) => { check(id); return {head:sha,branch:null,commits:[commit],nextOffset:null}; };
    port.checkoutCommit = async (_pair, id, revision) => {
      check(id); expect(revision).toBe(sha);
      return {body:'Changed the renderer.',initial:false,limited:false,files:[file]};
    };
    const seen: string[] = [];
    port.checkoutFile = async (_pair,id,request) => {
      check(id);expect(request.path).toBe(file.path); expect(request.head).toBe(sha);
      seen.push(request.base!);
      const prefix = ['zero','one','two','three','four','five','six','seven','eight'].join('\n')+'\n';
      return {kind:'text',original:prefix+'before\n',modified:prefix+'after\n'};
    };
    await app.render('/coding-agent',{search:{host:'1'},overrides:{'coding-agent':{t3Mobile:port}}});
    await userEvent.pointer({keys:'[MouseRight]',target:await screen.findByRole('button',{name:'Mobile checkout'})});
    await userEvent.click(await screen.findByRole('button',{name:'Source Control'}));
    expect(await screen.findByText('changed.ts')).toBeTruthy();
    expect(screen.getAllByText('aaaaaaa')).toHaveLength(2);
    await userEvent.click(screen.getByRole('button',{name:file.path}));
    expect(await screen.findByText('1 of 1 · 0 viewed')).toBeTruthy();
    await userEvent.click(await screen.findByRole('button',{name:'Expand 6 unchanged lines'}));
    expect(await screen.findByText('zero')).toBeTruthy();
    await userEvent.click(screen.getByRole('checkbox',{name:'Viewed'}));
    expect(screen.getByText('1 of 1 · 1 viewed')).toBeTruthy();
    await userEvent.click(screen.getByRole('button',{name:'Back to changed files'}));
    await userEvent.click(screen.getByRole('button',{name:'Commits'}));
    await userEvent.click(await screen.findByRole('button',{name:'Selected change'}));
    expect(await screen.findByText('Changed the renderer.')).toBeTruthy();
    expect(await screen.findByText('1 file')).toBeTruthy();
    expect(screen.getByText('Developer')).toBeTruthy();
    await userEvent.click(screen.getByRole('button',{name:file.path}));
    expect(await screen.findByText('1 of 1 in this commit')).toBeTruthy();
    expect(await screen.findByRole('button',{name:'Expand 6 unchanged lines'})).toBeTruthy();
    expect(seen).toEqual(['c'.repeat(40),commit.parent]);
    await userEvent.click(screen.getByRole('button',{name:'Next file'}));
    expect(screen.getByText('1 of 1 in this commit')).toBeTruthy();
    expect(screen.getByRole('button',{name:'Expand 6 unchanged lines'})).toBeTruthy();
    await userEvent.click(screen.getByRole('button',{name:'Back to changed files'}));
    await userEvent.click(screen.getByRole('button',{name:'Back to commits'}));
    expect(await screen.findByRole('button',{name:'Selected change'})).toBeTruthy();
    await userEvent.click(screen.getByRole('button',{name:'Back to source control'}));
    expect(await screen.findByText('changed.ts')).toBeTruthy();
    await userEvent.click(screen.getByRole('button',{name:'Back to workspaces'}));
    expect(await screen.findByRole('button',{name:'Mobile checkout'})).toBeTruthy();
  });

  it('Given a checkout, when the reader marks it unread and then read, then the reopened menu offers the reverse action', async () => {
    const port = backend();
    await app.render('/coding-agent', {search:{host:'1'}, overrides:{'coding-agent':{t3Mobile:port}}});
    const openMenu = async () => userEvent.pointer({keys:'[MouseRight]', target:await screen.findByRole('button', {name:'Mobile checkout'})});
    await openMenu();
    await userEvent.click(await screen.findByRole('button',{name:'Mark Unread'}));
    await openMenu();
    await userEvent.click(await screen.findByRole('button',{name:'Mark Read'}));
    await openMenu();
    expect(await screen.findByRole('button',{name:'Mark Unread'})).toBeTruthy();
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

  it('Given a paired chat, when adding a workspace file and then a skill, then each selection is inserted into the draft', async () => {
    const port = backend();
    const connect = port.open;
    port.open = async (...args) => ({
      ...await connect(...args),
      files: async () => [{ kind: 'file', token: 'notes/my plan.md' }],
      catalog: async () => [
        { kind: 'skill', token: '/review', description: 'Review a change' },
        { kind: 'command', token: '/status' },
      ],
    });
    await app.render('/coding-agent/thread-42', { overrides: { 'coding-agent': { t3Mobile: port } } });
    await userEvent.click(await screen.findByRole('button', { name: 'Add to message' }));
    await userEvent.click(await screen.findByRole('button', { name: 'Workspace file' }));
    await userEvent.click(await screen.findByRole('button', { name: 'notes/my plan.md' }));
    const field = screen.getByPlaceholderText('Ask anything, @ to mention files, / for commands') as HTMLTextAreaElement;
    expect(field.value).toBe('@"notes/my plan.md" ');
    await userEvent.click(screen.getByRole('button', { name: 'Add to message' }));
    await userEvent.click(await screen.findByRole('button', { name: 'Skills' }));
    await userEvent.click(await screen.findByRole('button', { name: '/review' }));
    expect(field.value.trim()).toBe('/review @"notes/my plan.md"');
    expect(screen.queryByRole('button', { name: '/status' })).toBeNull();
  });

  it('Given a connected chat with a slow catalog, when /model is picked and sent, then its picker opens and no turn is dispatched', async () => {
    const port = backend();
    const connect = port.open;
    const sent: string[] = [];
    port.open = async (...args) => ({
      ...await connect(...args),
      commands: [{kind: 'command', token: '/model', description: 'Choose the model'}],
      catalog: () => new Promise(() => {}),
      send: async (_id, text) => { sent.push(text); },
    });
    await app.render('/coding-agent/thread-42', {overrides: {'coding-agent': {t3Mobile: port}}});
    await userEvent.click(await screen.findByRole('button', {name: 'Add to message'}));
    await userEvent.click(await screen.findByRole('button', {name: 'Commands'}));
    await userEvent.click(await screen.findByRole('button', {name: '/model'}));
    const field = screen.getByPlaceholderText('Ask anything, @ to mention files, / for commands') as HTMLTextAreaElement;
    expect(field.value.trim()).toBe('/model');
    expect(screen.queryByRole('button', {name: /^Default/})).toBeNull();
    await userEvent.click(screen.getByRole('button', {name: 'Send message'}));
    expect(await screen.findByRole('button', {name: /^Default/})).toBeTruthy();
    expect(field.value).toBe('');
    expect(sent).toEqual([]);
    await userEvent.click(screen.getByRole('button', {name: 'Done'}));
    expect(screen.queryByRole('button', {name: /^Default/})).toBeNull();
  });

  it('Given a structured chat, when picking or submitting /resume, then the notice preserves the draft and sends no turn', async () => {
    const port = backend();
    const open = port.open;
    const sent: string[] = [];
    port.open = async (...args) => ({
      ...await open(...args), commands: [resumeCommand],
      catalog: async () => [{ kind: 'skill', token: '/resume' }],
      send: async (_id, text) => { sent.push(text); },
    });
    await app.render('/coding-agent/thread-42', { overrides: { 'coding-agent': { t3Mobile: port } } });
    const field = await screen.findByPlaceholderText('Ask anything, @ to mention files, / for commands') as HTMLTextAreaElement;
    await userEvent.click(await screen.findByRole('button', { name: 'Add to message' }));
    await userEvent.click(await screen.findByRole('button', { name: 'Commands' }));
    await userEvent.click(await screen.findByRole('button', { name: '/resume' }));
    expect(screen.getByRole('alert').textContent).toBe('Available in the desktop app');
    expect(field.value).toBe('/');
    await userEvent.type(field, 'RESUME example');
    const send = screen.getByRole('button', { name: 'Send message' });
    await userEvent.click(send);
    expect(field.value).toBe('/RESUME example');
    expect(sent).toEqual([]);

    await userEvent.clear(field);
    await userEvent.click(screen.getByRole('button', { name: 'Add to message' }));
    await userEvent.click(await screen.findByRole('button', { name: 'Skills' }));
    await userEvent.click(await screen.findByRole('button', { name: '/resume' }));
    await userEvent.type(field, 'this file');
    await userEvent.click(send);
    expect(sent).toEqual(['/resume this file']);
    expect(field.value).toBe('');
    await userEvent.type(field, '/resume');
    await userEvent.click(send);
    expect(sent).toEqual(['/resume this file']);
    expect(field.value).toBe('/resume');
  });

  it('Given effort support changes with the model, when submitting /effort, then unavailable drafts survive and supported choices use the model option', async () => {
    const port = backend();
    const open = port.open;
    const setModel = vi.fn().mockResolvedValue(undefined);
    const send = vi.fn();
    const option = { id: 'agent-intensity', current: 'medium', choices: [{slug: 'max', name: 'Extra high'}] };
    const models = vi.fn().mockResolvedValueOnce([]).mockResolvedValue([{slug: 'model-a', name: 'Model A', effort: option}]);
    port.open = async (pair, id, update, disconnected) => {
      const session = await open(pair, id, snapshot => update({
        ...snapshot,
        thread: {...snapshot.thread, modelSelection: {instanceId: 'opencode', model: 'model-a'}},
      }), disconnected);
      return {...session, models, setModel, send};
    };
    await app.render('/coding-agent/thread-42', {overrides: {'coding-agent': {t3Mobile: port}}});
    const field = await screen.findByPlaceholderText('Ask anything, @ to mention files, / for commands') as HTMLTextAreaElement;
    await userEvent.type(field, '/effort');
    const submit = screen.getByRole('button', {name: 'Send message'});
    await userEvent.click(submit);
    expect(await screen.findByText('Could not open the effort picker.')).toBeTruthy();
    expect(field.value).toBe('/effort');
    await userEvent.click(submit);
    const choice = await screen.findByRole('button', {name: 'Extra high'});
    expect(field.value).toBe('');
    await userEvent.click(choice);
    expect(setModel).toHaveBeenCalledWith('new-message', 'opencode', 'model-a', {id: 'agent-intensity', value: 'max'});
    await userEvent.click(screen.getByRole('button', {name: 'Done'}));
    await userEvent.type(field, '/effort EXTRA HIGH');
    await userEvent.click(submit);
    expect(setModel).toHaveBeenCalledTimes(2);
    expect(field.value).toBe('');
    await userEvent.type(field, '/effort impossible');
    await userEvent.click(submit);
    expect(await screen.findByText('Could not apply effort impossible.')).toBeTruthy();
    expect(field.value).toBe('/effort impossible');
    expect(send).not.toHaveBeenCalled();
  });

  it('Given a model display name, when its command is refused and retried, then the draft stays until the selected model is saved', async () => {
    const port = backend();
    const connect = port.open;
    const changes: string[][] = [];
    const sent: string[] = [];
    port.open = async (...args) => ({
      ...await connect(...args),
      models: async () => [{slug: 'minimax/flash', name: 'MiniMax Flash'}],
      setModel: async (id, instance, model) => {
        changes.push([id, instance, model]);
        if (changes.length === 1) throw new Error('Host unavailable');
      },
      send: async (_id, text) => { sent.push(text); },
    });
    await app.render('/coding-agent/thread-42', {overrides: {'coding-agent': {t3Mobile: port}}});
    await screen.findByRole('button', {name: 'Model, Model'});
    const field = screen.getByPlaceholderText('Ask anything, @ to mention files, / for commands') as HTMLTextAreaElement;
    await userEvent.type(field, '/MODEL minimax flash');
    await userEvent.click(screen.getByRole('button', {name: 'Send message'}));
    expect(await screen.findByText('Host unavailable')).toBeTruthy();
    expect(field.value).toBe('/MODEL minimax flash');
    await userEvent.click(screen.getByRole('button', {name: 'Send message'}));
    await waitFor(() => expect(field.value).toBe(''));
    expect(changes).toEqual(Array(2).fill(['new-message', 'opencode', 'minimax/flash']));
    expect(sent).toEqual([]);
    await userEvent.type(field, 'Explain /model');
    await userEvent.click(screen.getByRole('button', {name: 'Send message'}));
    expect(sent).toEqual(['Explain /model']);
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
    const readCatalog = vi.fn(port.workspaces);
    port.workspaces = readCatalog;
    await app.render('/coding-agent', {
      overrides: { 'coding-agent': { t3Mobile: port } },
    });
    await userEvent.click(await screen.findByRole('button',{name:'Open Fixture Mac'}));
    expect(readCatalog).not.toHaveBeenCalled();
    expect(routerHarness().lastPush()).toBe('/coding-agent?host=1');
    await app.render('/coding-agent', {search:{host:'1'},overrides:{'coding-agent':{t3Mobile:port}}});
    await userEvent.click(await screen.findByRole('button',{name:'Mobile checkout'}));
    expect(readCatalog).toHaveBeenCalledTimes(1);
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
