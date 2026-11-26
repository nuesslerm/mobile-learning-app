import { useAgentComponents } from '@lib/agent-ui-contracts';
import { Logger } from '@lib/logging';
import { router } from 'expo-router';
import { useEffect, useRef, useState } from 'react';
import type { Pairing } from '../domain/mobile';
import type { T3MobilePort } from '../domain/t3Mobile';
import {
  chatRoute,
  desktopPlatform,
  type CheckoutSessions,
  type WorkspaceCatalog,
  type CheckoutRow,
  type CheckoutEdit,
} from '../domain/workspaceCatalog';
import type { WorkspacePreferences } from '../domain/workspacePreferences';
import { T3WorkspaceOptions } from './T3WorkspaceOptions';

import { T3SourceControl } from './T3SourceControl';

export function T3HostWorkspaces({
  api,
  pair,
  catalog,
  onUpdated,
  onCatalogUpdated,
  onBack,
}: {
  api: T3MobilePort;
  pair: Pairing;
  catalog: WorkspaceCatalog;
  onUpdated: (row: CheckoutRow) => void;
  onCatalogUpdated: (next: WorkspaceCatalog) => void;
  onBack: () => void;
}) {
  const { HostWorkspaceList, WorkspaceActions } = useAgentComponents();
  const [preferences, setPreferences] = useState<WorkspacePreferences>();
  const [search, setSearch] = useState('');
  const [selected, setSelected] =
    useState<WorkspaceCatalog['checkouts'][number]>();
  const [actionRow, setActionRow] = useState<CheckoutRow>();
  const [sourceCheckout, setSourceCheckout] = useState<CheckoutRow>();
  const [mutationError, setMutationError] = useState('');
  const [catalogError, setCatalogError] = useState<string>();
  const [saving, setSaving] = useState(false);
  const pending = useRef(false);
  const catalogRead = useRef<AbortController | undefined>(undefined);
  useEffect(() => () => catalogRead.current?.abort(), []);
  async function returnFromSource(changed: boolean) {
    setSourceCheckout(undefined);
    if (!changed) return;
    setCatalogError(undefined);
    catalogRead.current?.abort();
    const controller = new AbortController();
    catalogRead.current = controller;
    try {
      const result = await api.workspaces(pair, controller.signal);
      if (!controller.signal.aborted) onCatalogUpdated(result);
    } catch {
      if (!controller.signal.aborted) {
        Logger.warn('coding_agent.t3.checkout_refresh_failed');
        setCatalogError('Could not refresh the checkout.');
      }
    }
  }
  async function edit(patch: CheckoutEdit) {
    if (!actionRow || pending.current) return;
    catalogRead.current?.abort();
    pending.current = true;
    setSaving(true);
    setMutationError('');
    try {
      const saved = await api.editWorkspace(pair, actionRow.id, patch);
      onUpdated(saved);
      setActionRow(undefined);
    } catch {
      Logger.warn('coding_agent.t3.workspace_edit_failed');
      setMutationError('Could not save the change. Try again.');
    } finally {
      pending.current = false;
      setSaving(false);
    }
  }
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
  if (sourceCheckout) return <T3SourceControl api={api} pair={pair} checkout={sourceCheckout} onBack={(changed) => void returnFromSource(changed)} />;
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
  return (
    <>
      <HostWorkspaceList
        title={catalog.host.name}
        platform={desktopPlatform(catalog.host.os)}
        group={preferences?.groupBy === 'repo' ? catalog.repo : undefined}
        rows={rows.map((r) => ({
          id: r.id,
          name: r.name,
          pinned: r.pinned,
          unread: r.unread,
          detail:
            preferences?.cardLayout === 'compact'
              ? undefined
              : (r.branch ?? undefined),
        }))}
        onActions={(id) => {
          setMutationError('');
          setActionRow(catalog.checkouts.find((r) => r.id === id));
        }}
        onSelect={(id) => void open(id)}
        notice={
          catalogError ?? (rows.length
            ? undefined
            : query
              ? 'No matching workspaces.'
              : 'No workspaces.')
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
      {actionRow && (
        <WorkspaceActions
          key={actionRow.id}
          name={actionRow.name}
          branch={actionRow.branch}
          automaticName={actionRow.automaticName ?? actionRow.name}
          pinned={!!actionRow.pinned}
          unread={!!actionRow.unread}
          onSourceControl={() => {
            setSourceCheckout(actionRow);
            setActionRow(undefined);
          }}
          onToggleRead={() => void edit({ isUnread: !actionRow.unread })}
          saving={saving}
          error={mutationError}
          onClose={() => {
            if (!pending.current) setActionRow(undefined);
          }}
          onPin={() => void edit({ isPinned: !actionRow.pinned })}
          onRename={(name) =>
            name === actionRow.name
              ? setActionRow(undefined)
              : void edit({ displayName: name })
          }
        />
      )}
    </>
  );
}
