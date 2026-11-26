import { useAgentComponents, type HostWorkspaceListProps } from '@lib/agent-ui-contracts';
import { Logger } from '@lib/logging';
import { router } from 'expo-router';
import { useEffect, useRef, useState } from 'react';
import type { Pairing } from '../domain/mobile';
import type { T3MobilePort } from '../domain/t3Mobile';
import {
  chatRoute,
  resolveWorkspaceStatus,
  desktopPlatform,
  type CheckoutSessions,
  type WorkspaceCatalog,
  type CheckoutRow,
  type WorkspaceStatus,
  type CheckoutEdit,
} from '../domain/workspaceCatalog';
import type { WorkspacePreferences } from '../domain/workspacePreferences';
import { T3WorkspaceOptions } from './T3WorkspaceOptions';

import { useWorkspaceDisclosure } from '../state/useWorkspaceDisclosure';
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
  const disclosure = useWorkspaceDisclosure(api, pair);
  const [search, setSearch] = useState('');
  const [selected, setSelected] =
    useState<WorkspaceCatalog['checkouts'][number]>();
  const [actionRow, setActionRow] = useState<CheckoutRow>();
  const [sourceCheckout, setSourceCheckout] = useState<CheckoutRow>();
  const [mutationError, setMutationError] = useState('');
  const [statusPicker, setStatusPicker] = useState<{ choices?: WorkspaceStatus[]; selected?: string; failed: boolean }>();
  const statusRequest = useRef<AbortController | undefined>(undefined);
  useEffect(() => () => statusRequest.current?.abort(), []);
  const groupByStatus = preferences?.groupBy === 'workspace-status';
  const [statusGroups, setStatusGroups] = useState<WorkspaceStatus[]>();
  const [groupError, setGroupError] = useState(false);
  const [groupRetry, setGroupRetry] = useState(0);
  useEffect(() => {
    setStatusGroups(undefined);
    setGroupError(false);
    if (!groupByStatus) return;
    const controller = new AbortController();
    api.workspaceStatuses(pair, controller.signal).then(choices => {
      if (!controller.signal.aborted) setStatusGroups(choices);
    }).catch(cause => {
      if (controller.signal.aborted) return;
      Logger.withError(cause).warn('coding_agent.t3.status_groups_failed');
      setGroupError(true);
    });
    return () => controller.abort();
  }, [api, pair, groupByStatus, groupRetry]);

  async function openStatuses() {
    if (!actionRow) return;
    statusRequest.current?.abort();
    const controller = new AbortController();
    statusRequest.current = controller;
    setStatusPicker({ failed: false });
    setMutationError('');
    try {
      const choices = await api.workspaceStatuses(pair, controller.signal);
      if (controller.signal.aborted) return;
      if (groupByStatus) {
        setStatusGroups(choices);
        setGroupError(false);
      }
      const selected = resolveWorkspaceStatus(choices, actionRow.workspaceStatus);
      setStatusPicker({ choices, selected: selected?.id, failed: false });
    } catch (cause) {
      if (controller.signal.aborted) return;
      Logger.withError(cause).warn('coding_agent.t3.workspace_statuses_failed');
      setStatusPicker({ failed: true });
    }
  }
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
        router.push(chatRoute(found.active, row.name, row.id));
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
        sections={[{ key: 'chats', rows: (sessions?.sessions ?? []).map((s) => ({
          id: s.id,
          name: s.title,
        })) }]}
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
        onSelect={(id) => router.push(chatRoute(id, selected.name, selected.id))}
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
  type Section = HostWorkspaceListProps['sections'][number];
  const sections: Section[] = [];
  const pinned: Section = { key: 'pinned', label: 'Pinned', icon: 'pin', rows: [] };
  const byStatus = new Map<string, Section>();
  for (const status of statusGroups ?? []) {
    byStatus.set(status.id, { key: 'workspace-status:' + encodeURIComponent(status.id), label: status.label, rows: [] });
  }
  const projectGroup = preferences?.groupBy === 'repo';
  const other: Section = { key: projectGroup ? 'repo:' + catalog.repo : 'all', label: projectGroup ? catalog.repo : undefined,
    icon: projectGroup ? 'folder' : undefined, rows: [] };
  for (const row of rows) {
    const status = groupByStatus && statusGroups && resolveWorkspaceStatus(statusGroups, row.workspaceStatus);
    const section = row.pinned ? pinned : (status && byStatus.get(status.id)) || other;
    const detailed = preferences?.cardLayout !== 'compact';
    section.rows.push({
      id: row.id, name: row.name, unread: row.unread,
      project: detailed && groupByStatus ? catalog.repo : undefined,
      projectInTitle: !detailed && groupByStatus,
      detail: detailed ? row.branch ?? undefined : undefined,
    });
  }
  if (pinned.rows.length) sections.push(pinned);
  for (const section of byStatus.values()) {
    if (section.rows.length) sections.push(section);
  }
  if (other.rows.length || projectGroup) sections.push(other);
  return (
    <>
      <HostWorkspaceList
        title={catalog.host.name}
        platform={desktopPlatform(catalog.host.os)}
        sections={sections}
        disclosure={disclosure}
        retry={disclosure.error ? disclosure.reload : groupError ? () => setGroupRetry(n => n + 1) : undefined}
        onActions={(id) => {
          setMutationError('');
          statusRequest.current?.abort();
          setStatusPicker(undefined);
          setActionRow(catalog.checkouts.find((r) => r.id === id));
        }}
        onSelect={(id) => void open(id)}
        notice={
          catalogError ?? disclosure.error ?? (groupError ? 'Could not load status groups.' : undefined) ?? (rows.length
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
          statusPicker={statusPicker}
          onOpenStatuses={() => void openStatuses()}
          onStatus={id => void edit({ workspaceStatus: id })}
          onToggleRead={() => void edit({ isUnread: !actionRow.unread })}
          saving={saving}
          error={mutationError}
          onClose={() => {
            if (pending.current) return;
            statusRequest.current?.abort();
            setActionRow(undefined);
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
