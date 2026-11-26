import { describe, expect, it, vi } from 'vitest';
import { composerCatalog, composerFiles } from '../composerDiscovery';

describe('T3 composer discovery', () => {
  it('Given catalogs for two checkouts, when asking for Codex suggestions, then only invocable entries from this checkout are offered', async () => {
    const call = vi.fn(async () => ({
      providers: [{
        instanceId: 'work-codex', driver: 'codex',
        workspaceSnapshots: [
          { cwd: '/one', slashCommands: [{ name: 'wrong' }], skills: [] },
          { cwd: '/two', slashCommands: [{ name: 'info', input: { hint: 'topic' } }], skills: [
            { name: 'review', enabled: true },
            { name: 'hidden', enabled: true, userInvocable: false },
            { name: 'disabled', enabled: false },
          ] },
        ],
      }],
    }));
    const rows = await composerCatalog(call, '/two', 'work-codex');
    expect(call).toHaveBeenCalledWith('server.refreshProviders', { cwd: '/two', instanceId: 'work-codex', workspaceSnapshotsOnDemand: true, providerCatalogsOnDemand: true });
    expect(rows.map(row => [row.kind, row.token, row.hint])).toEqual([['command', '/info', 'topic'], ['skill', '$review', undefined]]);
  });

  it('Given a typed nested filename, when the server returns matching files, then their paths stay relative to that checkout', async () => {
    const call = vi.fn(async () => ({ entries: [{ kind: 'file', path: 'notes/plan 1.md' }] }));
    const rows = await composerFiles(call, '/work', 'notes/plan');
    expect(call).toHaveBeenCalledWith('projects.searchEntries', { cwd: '/work', query: 'notes/plan', kind: 'file', limit: 10 });
    expect(rows).toEqual([{ kind: 'file', token: 'notes/plan 1.md' }]);
  });
});
