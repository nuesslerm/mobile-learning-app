import { useAgentComponents } from '@lib/agent-ui-contracts';
import { Logger } from '@lib/logging';
import { router } from 'expo-router';
import { useEffect, useRef, useState } from 'react';
import type { Pairing } from '../domain/mobile';
import type { T3MobilePort } from '../domain/t3Mobile';
import {
  chatRoute,
  type CheckoutSessions,
  type WorkspaceCatalog,
} from '../domain/workspaceCatalog';
import type { WorkspacePreferences } from '../domain/workspacePreferences';
import { T3WorkspaceOptions } from './T3WorkspaceOptions';

export function T3HostWorkspaces({
  api,
  pair,
  catalog,
  onBack,
}: {
  api: T3MobilePort;
  pair: Pairing;
  catalog: WorkspaceCatalog;
  onBack: () => void;
}) {
  const { HostWorkspaceList } = useAgentComponents();
  const [preferences, setPreferences] = useState<WorkspacePreferences>();
  const [search, setSearch] = useState('');
  const [selected, setSelected] =
    useState<WorkspaceCatalog['checkouts'][number]>();
  const [sessions, setSessions] = useState<CheckoutSessions>();
  const [error, setError] = useState('');
  const request = useRef<AbortController | undefined>(undefined);
  useEffect(() => () => request.current?.abort(), []);
  const open = async (id: string) => {
    const row = catalog.checkouts.find((r) => r.id === id);
    if (!row) return;
    request.current?.abort();
    const controller = new AbortController();
    request.current = controller;
    setSelected(row);
    setSessions(undefined);
    setError('');
    try {
      const found = await api.checkoutSessions(pair, id, controller.signal);
      if (controller.signal.aborted) return;
      if (found.active) {
        setSelected(undefined);
        router.push(chatRoute(found.active, row.name));
      } else setSessions(found);
    } catch {
      if (controller.signal.aborted) return;
      Logger.warn('coding_agent.t3.checkout_open_failed');
      setError('Could not load workspace chats.');
    }
  };
  if (selected)
    return (
      <HostWorkspaceList
        key={selected.id}
        title={selected.name}
        rows={(sessions?.sessions ?? []).map((s) => ({
          id: s.id,
          name: s.title,
        }))}
        notice={
          error ||
          (!sessions
            ? 'Opening workspace…'
            : sessions.sessions.length
              ? undefined
              : 'No chats in this workspace.')
        }
        retry={error ? () => void open(selected.id) : undefined}
        onBack={() => {
          request.current?.abort();
          setSelected(undefined);
        }}
        onSelect={(id) => router.push(chatRoute(id, selected.name))}
      />
    );
  const filters = preferences?.filters;
  const query = search.trim().toLowerCase();
  const rows = catalog.checkouts.filter(
    (r) =>
      (!filters?.filterRepoIds.length ||
        filters.filterRepoIds.includes(catalog.repo)) &&
      !(filters?.hideDefaultBranchWorkspace && r.main) &&
      !(filters?.hideDetachedHeadWorkspaces && !r.branch) &&
      (r.name.toLowerCase().includes(query) ||
        r.branch?.toLowerCase().includes(query))
  );
  if (preferences?.sortBy === 'name')
    rows.sort((a, b) => a.name.localeCompare(b.name));
  else if (preferences?.sortBy !== 'manual')
    rows.sort(
      (a, b) => Number(b.main) - Number(a.main) || a.name.localeCompare(b.name)
    );
  const os =
    (
      {
        darwin: 'macOS',
        macos: 'macOS',
        linux: 'Linux',
        win32: 'Windows',
      } as Record<string, string>
    )[catalog.host.os] ?? catalog.host.os;
  return (
    <HostWorkspaceList
      title={catalog.host.name}
      platform={os}
      group={preferences?.groupBy === 'repo' ? catalog.repo : undefined}
      rows={rows.map((r) => ({
        id: r.id,
        name: r.name,
        detail:
          preferences?.cardLayout === 'compact'
            ? undefined
            : (r.branch ?? undefined),
      }))}
      onSelect={(id) => void open(id)}
      notice={
        rows.length
          ? undefined
          : query
            ? 'No matching workspaces.'
            : 'No workspaces.'
      }
      search={{ value: search, onChange: setSearch }}
      onBack={onBack}
      options={
        <T3WorkspaceOptions
          api={api}
          pair={pair}
          projects={[{ id: catalog.repo, title: catalog.repo }]}
          onChanged={setPreferences}
        />
      }
    />
  );
}
