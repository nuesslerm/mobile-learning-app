import {
  useAgentComponents,
  type HomeAgentSectionProps,
} from '@lib/agent-ui-contracts';
import { Logger } from '@lib/logging';
import { router, useFocusEffect } from 'expo-router';
import { useCallback, useEffect, useRef, useState } from 'react';
import type { Pairing } from '../../domain/mobile';
import type { T3MobilePort } from '../../domain/t3Mobile';
import { chatRoute, type DesktopAgents } from '../../domain/workspaceCatalog';

const statusRank = {
  waiting: 0,
  blocked: 1,
  failed: 2,
  working: 3,
  interrupted: 4,
  done: 5,
};
const statusGroup = {
  waiting: 'Needs you',
  blocked: 'Needs you',
  failed: 'Needs you',
  working: 'Working',
  interrupted: 'Done',
  done: 'Done',
};

function age(time: number, now: number) {
  if (!time) return '';
  const minutes = Math.max(0, Math.floor((now - time) / 60000));
  if (!minutes) return 'just now';
  if (minutes < 60) return minutes + 'm';
  if (minutes < 1440) return Math.floor(minutes / 60) + 'h';
  return Math.floor(minutes / 1440) + 'd';
}

export function PairedAgents({
  api,
  pair,
  computer,
  active,
  query,
  unreadOnly,
  groupBy,
}: {
  api: T3MobilePort;
  pair: Pairing;
  computer?: string | null;
  active: boolean;
  query: string;
  unreadOnly: boolean;
  groupBy: string;
}) {
  const { HomeEmptyState, HomeAgentSection } = useAgentComponents();
  const [catalog, setCatalog] = useState<DesktopAgents>();
  const [error, setError] = useState('');
  const selection = useRef<AbortController | undefined>(undefined);
  useEffect(() => () => selection.current?.abort(), []);
  useFocusEffect(
    useCallback(() => {
      if (!active) return;
      const controller = new AbortController();
      setError('');
      api
        .agents(pair, controller.signal)
        .then((next) => {
          if (!controller.signal.aborted) setCatalog(next);
        })
        .catch(() => {
          if (!controller.signal.aborted) {
            Logger.warn('coding_agent.t3.agents_list_failed');
            setError('Could not load agents from this desktop.');
          }
        });
      return () => {
        controller.abort();
        selection.current?.abort();
      };
    }, [active, api, pair])
  );

  const open = async (id: string) => {
    const row = catalog?.agents.find((agent) => agent.threadId === id);
    const place = row && catalog?.checkouts[row.workspace];
    if (!place) return;
    selection.current?.abort();
    const controller = new AbortController();
    selection.current = controller;
    setError('');
    try {
      const sessions = await api.checkoutSessions(
        pair,
        place.id,
        controller.signal
      );
      if (controller.signal.aborted) return;
      const target =
        sessions.sessions.find((session) => session.id === id)?.id ??
        sessions.active;
      if (target) router.push(chatRoute(target, place.name));
      else setError('No saved chat is available in this checkout.');
    } catch {
      if (!controller.signal.aborted) {
        Logger.warn('coding_agent.t3.agent_open_failed');
        setError('Could not open this agent.');
      }
    }
  };

  if (!catalog)
    return <HomeEmptyState icon="agents" title={error || 'Loading agents…'} />;
  const now = Date.now();
  const sections = new Map<string, HomeAgentSectionProps['rows']>();
  const needle = query.toLowerCase();
  const rows =
    groupBy === 'Status'
      ? [...catalog.agents].sort(
          (a, b) =>
            statusRank[a.status] - statusRank[b.status] ||
            b.updatedAt - a.updatedAt
        )
      : catalog.agents;
  for (const row of rows) {
    const name = catalog.checkouts[row.workspace]?.name;
    if (
      !name ||
      (unreadOnly && !row.unread) ||
      (needle &&
        ![row.title, name, catalog.repo, computer ?? ''].some((s) =>
          s.toLowerCase().includes(needle)
        ))
    )
      continue;
    const group =
      groupBy === 'Status'
        ? statusGroup[row.status]
        : groupBy === 'Workspace'
          ? name
          : groupBy === 'Project'
            ? catalog.repo
            : groupBy === 'Desktop'
              ? computer || catalog.repo
              : 'Other';
    const groupRows = sections.get(group) ?? [];
    groupRows.push({
      id: row.threadId,
      title: row.title,
      place: name,
      unread: row.unread,
      status: row.status,
      age: age(row.updatedAt, now),
    });
    sections.set(group, groupRows);
  }
  return (
    <>
      {error && <HomeEmptyState icon="agents" title={error} />}
      {sections.size ? (
        [...sections].map(([title, group]) => (
          <HomeAgentSection
            key={title}
            title={title}
            rows={group}
            onSelect={(id) => void open(id)}
          />
        ))
      ) : (
        <HomeEmptyState
          icon="agents"
          title={
            query ? 'No agents match “' + query + '”' : 'No agents running'
          }
          message={
            query
              ? undefined
              : 'Start an agent in any workspace and it shows up here.'
          }
          iconSize={30}
          titleSize={16}
          bodySize={14}
        />
      )}
    </>
  );
}
