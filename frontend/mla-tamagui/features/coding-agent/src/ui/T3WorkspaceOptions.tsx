import { Logger } from '@lib/logging';
import { useEffect, useRef, useState } from 'react';
import {
  useAgentComponents,
  type WorkspaceOptionGroup,
  type WorkspaceOptionRow,
} from '@lib/agent-ui-contracts';
import type { Pairing } from '../domain/mobile';
import type { T3MobilePort, T3Shell } from '../domain/t3Mobile';
import type {
  WorkspacePatch,
  WorkspacePreferences,
} from '../domain/workspacePreferences';

const groups = [
  ['none', 'None'],
  ['workspace-status', 'Status'],
  ['pr-status', 'PR'],
  ['repo', 'Project'],
] as const;
const sorts = [
  ['name', 'Name'],
  [
    'smart',
    'Agent Activity',
    'Agents that need attention, then most recent activity.',
  ],
  ['recent', 'Recent'],
  ['repo', 'Project'],
  ['manual', 'Manual', 'Drag workspaces to arrange them within each group.'],
] as const;
const orders = [
  ['manual', 'Manual', 'Drag projects to arrange them'],
  ['recent', 'Recent', 'Most recent workspace activity'],
] as const;
const layouts = [
  ['detailed', 'Detailed'],
  ['compact', 'Compact'],
] as const;
const activity = [
  ['compact', 'Compact'],
  ['full', 'Full list'],
] as const;
const properties = [
  ['issue', 'GitHub ticket'],
  ['linear-issue', 'Linear issue'],
  ['jira-issue', 'Jira issue'],
  ['pr', 'PR/MR link'],
  ['automation', 'Automation'],
  ['comment', 'Notes'],
  ['inline-agents', 'Agent activity'],
  ['host', 'Host'],
] as const;
const filterChoices = [
  ['hideSleepingWorkspaces', 'Hide sleeping'],
  ['alwaysShowDefaultBranchWorkspace', 'Except default branch'],
  ['hideDefaultBranchWorkspace', 'Hide default branch'],
  ['hideAutomationGeneratedWorkspaces', 'Hide automation-created'],
  ['hideCliCreatedWorkspaces', 'Hide CLI-created'],
  ['hideDetachedHeadWorkspaces', 'Hide detached HEAD'],
] as const;
const clearFilters: WorkspacePreferences['filters'] = {
  hideSleepingWorkspaces: false,
  alwaysShowDefaultBranchWorkspace: true,
  hideDefaultBranchWorkspace: false,
  hideAutomationGeneratedWorkspaces: false,
  hideCliCreatedWorkspaces: false,
  hideDetachedHeadWorkspaces: false,
  hideWorkspacesFromOtherDevices: false,
  filterRepoIds: [],
  visibleHostIds: null,
};
type Page =
  | 'Sort by'
  | 'Project order'
  | 'Card layout'
  | 'Show properties'
  | 'Projects';
type Choices<T extends string> = readonly (readonly [T, string, string?])[];

function select<T extends string>(
  choices: Choices<T>,
  selected: T,
  change: (id: T) => void
): WorkspaceOptionRow[] {
  return choices.map(([id, label, description]) => ({
    label,
    description,
    kind: 'radio',
    selected: id === selected,
    onPress: () => change(id),
  }));
}
function name(choices: Choices<string>, id: string) {
  return choices.find((choice) => choice[0] === id)?.[1];
}
function toggle(ids: string[], id: string) {
  return ids.includes(id) ? ids.filter((value) => value !== id) : [...ids, id];
}

export function T3WorkspaceOptions({
  api,
  pair,
  projects,
  onChanged,
}: {
  api: T3MobilePort;
  pair: Pairing;
  projects: T3Shell['projects'];
  onChanged: (settings: WorkspacePreferences) => void;
}) {
  const { WorkspaceOptions } = useAgentComponents();
  const [visible, setVisible] = useState(false);
  const [page, setPage] = useState<Page>();
  const [value, setValue] = useState<WorkspacePreferences>();
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const pending = useRef<Promise<void> | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  useEffect(() => {
    const controller = new AbortController();
    async function load() {
      setLoading(true);
      setError('');
      try {
        await pending.current;
        if (controller.signal.aborted) return;
        const result = await api.workspacePreferences(pair, controller.signal);
        if (!controller.signal.aborted) {
          setValue(result);
          onChanged(result);
        }
      } catch {
        if (!controller.signal.aborted) {
          Logger.warn('coding_agent.workspace_preferences.load_failed');
          setError('Could not load workspace options.');
        }
      } finally {
        if (!controller.signal.aborted) setLoading(false);
      }
    }
    void load();
    return () => controller.abort();
  }, [api, pair, refresh, onChanged]);

  function save(patch: WorkspacePatch) {
    if (pending.current) return;
    setSaving(true);
    setError('');
    pending.current = api
      .saveWorkspacePreferences(pair, patch)
      .then((result) => {
        if (alive.current) {
          setValue(result);
          onChanged(result);
        }
      })
      .catch(() => {
        Logger.warn('coding_agent.workspace_preferences.save_failed');
        if (alive.current) setError('Could not save workspace options.');
      })
      .finally(() => {
        pending.current = null;
        if (alive.current) setSaving(false);
      });
  }
  const root: WorkspaceOptionGroup[] = [];
  const pages: Partial<Record<Page, WorkspaceOptionGroup[]>> = {};
  const link = (label: Page, value?: string): WorkspaceOptionRow => ({
    label,
    value,
    kind: 'page',
    onPress: () => setPage(label),
  });
  let resetEnabled = false;
  if (value) {
    const filters = value.filters;
    const changedFilters = filterChoices.some(
      ([key]) => filters[key] !== clearFilters[key]
    );
    resetEnabled =
      changedFilters ||
      filters.hideWorkspacesFromOtherDevices ||
      filters.filterRepoIds.length > 0 ||
      filters.visibleHostIds !== null;
    if (projects.length > 1) {
      const selected = filters.filterRepoIds;
      const names = projects.filter((project) => selected.includes(project.id));
      const summary =
        selected.length === 0
          ? 'All projects'
          : names.length === 1
            ? names[0].title
            : names.length + ' of ' + projects.length;
      root.push({ rows: [link('Projects', summary)] });
      pages.Projects = [
        {
          rows: [
            {
              label: 'All projects',
              kind: 'checkbox',
              selected: selected.length === 0,
              onPress: () => save({ filters: { filterRepoIds: [] } }),
            },
            ...projects.map((project) => ({
              label: project.title,
              kind: 'checkbox' as const,
              selected: selected.includes(project.id),
              onPress: () =>
                save({
                  filters: { filterRepoIds: toggle(selected, project.id) },
                }),
            })),
          ],
        },
      ];
    }
    root.push({
      label: 'Group by',
      segments: true,
      rows: select(groups, value.groupBy, (groupBy) => save({ groupBy })),
    });
    const rows = [link('Sort by', name(sorts, value.sortBy))];
    if (value.groupBy === 'repo')
      rows.push(link('Project order', name(orders, value.projectOrderBy)));
    rows.push(
      link('Card layout', name(layouts, value.cardLayout)),
      link(
        'Show properties',
        String(
          properties.filter(([id]) => value.worktreeCardProperties.includes(id))
            .length
        )
      )
    );
    root.push({ rows });
    root.push({
      label: 'Filters',
      rows: filterChoices
        .filter(
          ([key]) =>
            key !== 'alwaysShowDefaultBranchWorkspace' ||
            filters.hideSleepingWorkspaces
        )
        .map(([key, label]) => ({
          label,
          kind: 'switch',
          selected: filters[key],
          onPress: () => save({ filters: { [key]: !filters[key] } }),
        })),
    });
    pages['Sort by'] = [
      { rows: select(sorts, value.sortBy, (sortBy) => save({ sortBy })) },
    ];
    pages['Project order'] = [
      {
        rows: select(orders, value.projectOrderBy, (projectOrderBy) =>
          save({ projectOrderBy })
        ),
      },
    ];
    pages['Card layout'] = [
      {
        rows: select(layouts, value.cardLayout, (cardLayout) =>
          save({ cardLayout })
        ),
      },
    ];
    pages['Show properties'] = [
      {
        rows: properties.map(([id, label]) => ({
          label,
          kind: 'checkbox',
          selected: value.worktreeCardProperties.includes(id),
          onPress: () =>
            save({
              worktreeCardProperties: toggle(value.worktreeCardProperties, id),
            }),
        })),
      },
      {
        label: 'Agent activity layout',
        segments: true,
        rows: select(
          activity,
          value.agentActivityDisplayMode,
          (agentActivityDisplayMode) => save({ agentActivityDisplayMode })
        ),
      },
    ];
  }
  return (
    <WorkspaceOptions
      visible={visible}
      page={page}
      groups={root}
      pageGroups={page ? (pages[page] ?? []) : []}
      saving={saving || loading}
      loading={loading && !value}
      error={error}
      resetEnabled={resetEnabled}
      onOpen={() => {
        setVisible(true);
        setRefresh((x) => x + 1);
      }}
      onClose={() => {
        setVisible(false);
        setPage(undefined);
      }}
      onBack={() => setPage(undefined)}
      onReset={() => save({ filters: clearFilters })}
      onRetry={() => setRefresh((x) => x + 1)}
    />
  );
}
