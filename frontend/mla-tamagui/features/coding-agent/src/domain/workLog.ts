// features/coding-agent/src/domain/workLog.ts
//
// Pure derivation of the work-log rows for a thread's tool activities. Ported
// from references/t3code/apps/mobile/src/lib/threadActivity.ts over our
// `OrchestrationThread` shapes: no `effect`, no IO. Each tool activity is
// sniffed (command / file path / url / mcp server) into a `WorkLogEntry`;
// adjacent entries with the same collapse key merge, and overflow of a group
// folds behind a `work-toggle` row.

import type { OrchestrationThreadActivity } from './orchestration';

import type { ThreadFeedEntry } from './threadFeed';

export type WorkLogKind =
  | 'command'
  | 'file_change'
  | 'file_read'
  | 'web_search'
  | 'mcp'
  | 'tool'
  | 'diff';

export type WorkLogStatus = 'running' | 'done' | 'failed' | 'declined';

export interface WorkLogEntry {
  id: string;
  kind: WorkLogKind;
  title: string;
  detail?: string;
  status: WorkLogStatus;
  diff?: string;
  output?: string;
}

export const MAX_VISIBLE_WORK_LOG_ENTRIES = 1;

interface McpIdentity {
  server: string;
  tool: string;
}

interface DerivingEntry {
  activityKind: string;
  collapseKey: string | undefined;
  entry: WorkLogEntry;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === 'object'
    ? (value as Record<string, unknown>)
    : null;
}

function asTrimmedString(value: unknown): string | null {
  if (typeof value !== 'string') {
    return null;
  }
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function formatCommandValue(value: unknown): string | null {
  const direct = asTrimmedString(value);
  if (direct !== null) {
    return direct;
  }
  if (!Array.isArray(value)) {
    return null;
  }
  const parts: string[] = [];
  for (const entry of value) {
    const part = asTrimmedString(entry);
    if (part !== null) {
      parts.push(part);
    }
  }
  return parts.length > 0 ? parts.join(' ') : null;
}

function stripTrailingExitCode(value: string): string {
  return value.replace(/<exited with exit code \d+>\s*$/i, '').trim();
}

function extractCommand(
  payload: Record<string, unknown> | null
): string | null {
  const data = asRecord(payload?.data);
  const item = asRecord(data?.item);
  const candidates: unknown[] = [
    item?.command,
    asRecord(item?.input)?.command,
    asRecord(item?.result)?.command,
    asRecord(data?.input)?.command,
    data?.command,
    payload?.command,
    payload?.itemType === 'command_execution' &&
    typeof payload.detail === 'string'
      ? stripTrailingExitCode(payload.detail)
      : null,
  ];
  for (const candidate of candidates) {
    const command = formatCommandValue(candidate);
    if (command !== null) {
      return command;
    }
  }
  return null;
}

function extractChangedFiles(
  payload: Record<string, unknown> | null
): string[] {
  const data = asRecord(payload?.data);
  const direct = Array.isArray(data?.changedFiles)
    ? data?.changedFiles
    : payload?.changedFiles;
  if (Array.isArray(direct)) {
    const files: string[] = [];
    for (const entry of direct) {
      const file = asTrimmedString(entry);
      if (file !== null) {
        files.push(file);
      }
    }
    return files;
  }
  const files: string[] = [];
  const seen = new Set<string>();
  const collect = (value: unknown, depth: number): void => {
    if (depth > 3 || files.length >= 12) {
      return;
    }
    if (Array.isArray(value)) {
      for (const entry of value) {
        collect(entry, depth + 1);
        if (files.length >= 12) {
          return;
        }
      }
      return;
    }
    const record = asRecord(value);
    if (record === null) {
      return;
    }
    for (const key of [
      'path',
      'filePath',
      'relativePath',
      'filename',
      'newPath',
      'oldPath',
    ]) {
      const file = asTrimmedString(record[key]);
      if (file !== null && !seen.has(file)) {
        seen.add(file);
        files.push(file);
      }
    }
    for (const key of [
      'item',
      'result',
      'input',
      'changes',
      'files',
      'edits',
      'patch',
      'operations',
    ]) {
      if (key in record) {
        collect(record[key], depth + 1);
      }
    }
  };
  collect(data, 0);
  return files;
}

function sniffMcpIdentity(
  payload: Record<string, unknown> | null
): McpIdentity | null {
  const data = asRecord(payload?.data);
  const item = asRecord(data?.item);
  const server =
    asTrimmedString(item?.server) ??
    asTrimmedString(data?.serverName) ??
    asTrimmedString(data?.mcpServerId) ??
    asTrimmedString(data?.server);
  const tool =
    asTrimmedString(item?.tool) ??
    asTrimmedString(item?.toolName) ??
    asTrimmedString(data?.toolName) ??
    asTrimmedString(data?.name);
  if (server === null && tool === null) {
    return null;
  }
  return { server: server ?? '', tool: tool ?? '' };
}

function sniffUrl(payload: Record<string, unknown> | null): string | null {
  const data = asRecord(payload?.data);
  const item = asRecord(data?.item);
  return (
    asTrimmedString(item?.url) ??
    asTrimmedString(data?.url) ??
    asTrimmedString(payload?.url)
  );
}

function sniffWorkLogKind(
  payload: Record<string, unknown> | null,
  command: string | null,
  files: readonly string[],
  mcp: McpIdentity | null,
  url: string | null
): WorkLogKind {
  const itemType = asTrimmedString(payload?.itemType);
  if (itemType === 'file_read') {
    return 'file_read';
  }
  if (itemType === 'mcp_tool_call' || mcp !== null) {
    return 'mcp';
  }
  if (itemType === 'web_search' || url !== null) {
    return 'web_search';
  }
  if (itemType === 'command_execution' || command !== null) {
    return 'command';
  }
  if (itemType === 'file_change' || files.length > 0) {
    return 'file_change';
  }
  return 'tool';
}

function workLogStatus(
  activity: OrchestrationThreadActivity,
  payload: Record<string, unknown> | null
): WorkLogStatus {
  const status = asTrimmedString(payload?.status);
  if (status === 'inProgress') {
    return 'running';
  }
  if (status === 'failed') {
    return 'failed';
  }
  if (status === 'declined') {
    return 'declined';
  }
  if (status === 'completed') {
    return 'done';
  }
  if (activity.kind === 'tool.failed') {
    return 'failed';
  }
  if (activity.kind === 'tool.declined') {
    return 'declined';
  }
  if (activity.kind === 'tool.started') {
    return 'running';
  }
  if (
    activity.kind.endsWith('.completed') ||
    activity.kind.endsWith('.resolved')
  ) {
    return 'done';
  }
  return 'running';
}

function workLogDetail(
  kind: WorkLogKind,
  payload: Record<string, unknown> | null,
  command: string | null,
  files: readonly string[],
  mcp: McpIdentity | null,
  url: string | null
): string | undefined {
  if (kind === 'command' && command !== null) {
    return command;
  }
  if (kind === 'file_change' || kind === 'file_read') {
    if (files.length > 0) {
      return files.join(', ');
    }
  }
  if (kind === 'web_search') {
    const query =
      asTrimmedString(asRecord(payload?.data)?.query) ??
      asTrimmedString(payload?.detail);
    return url ?? query ?? undefined;
  }
  if (kind === 'mcp' && mcp !== null) {
    if (mcp.server.length > 0 && mcp.tool.length > 0) {
      return `${mcp.server}.${mcp.tool}`;
    }
    return mcp.server.length > 0 ? mcp.server : mcp.tool;
  }
  return asTrimmedString(payload?.detail) ?? undefined;
}

function workLogOutput(
  payload: Record<string, unknown> | null
): string | undefined {
  const data = asRecord(payload?.data);
  const item = asRecord(data?.item);
  const result = asRecord(item?.result);
  return (
    asTrimmedString(data?.output) ??
    asTrimmedString(item?.output) ??
    asTrimmedString(item?.aggregatedOutput) ??
    asTrimmedString(result?.output) ??
    asTrimmedString(payload?.output) ??
    undefined
  );
}

function workLogDiff(
  payload: Record<string, unknown> | null
): string | undefined {
  const data = asRecord(payload?.data);
  const patch =
    typeof data?.patch === 'string'
      ? data.patch
      : typeof payload?.patch === 'string'
        ? payload.patch
        : undefined;
  if (patch !== undefined && patch.trim().length > 0) {
    return patch;
  }
  return undefined;
}

function deriveCollapseKey(
  kind: WorkLogKind,
  command: string | null,
  files: readonly string[],
  mcp: McpIdentity | null,
  url: string | null
): string | undefined {
  if (kind === 'command' && command !== null) {
    return `command:${command}`;
  }
  if ((kind === 'file_change' || kind === 'file_read') && files.length > 0) {
    return `files:${files.join('\u001f')}`;
  }
  if (kind === 'web_search' && url !== null) {
    return `web:${url}`;
  }
  if (kind === 'mcp' && mcp !== null) {
    return `mcp:${mcp.server}.${mcp.tool}`;
  }
  return undefined;
}

function toDerivingEntry(activity: OrchestrationThreadActivity): DerivingEntry {
  const payload = asRecord(activity.payload);
  const command = extractCommand(payload);
  const files = extractChangedFiles(payload);
  const mcp = sniffMcpIdentity(payload);
  const url = sniffUrl(payload);
  const kind = sniffWorkLogKind(payload, command, files, mcp, url);
  const detail = workLogDetail(kind, payload, command, files, mcp, url);
  const diff = workLogDiff(payload);
  const output = workLogOutput(payload);
  const entry: WorkLogEntry = {
    id: activity.id,
    kind,
    title: asTrimmedString(payload?.title) ?? activity.summary,
    status: workLogStatus(activity, payload),
    ...(detail !== undefined ? { detail } : {}),
    ...(diff !== undefined ? { diff } : {}),
    ...(output !== undefined ? { output } : {}),
  };
  return {
    activityKind: activity.kind,
    entry,
    collapseKey: deriveCollapseKey(kind, command, files, mcp, url),
  };
}

function compareActivities(
  left: OrchestrationThreadActivity,
  right: OrchestrationThreadActivity
): number {
  return (
    left.createdAt.localeCompare(right.createdAt) ||
    (left.sequence ?? 0) - (right.sequence ?? 0) ||
    left.id.localeCompare(right.id)
  );
}

function isWorkToolActivity(activity: OrchestrationThreadActivity): boolean {
  if (activity.tone === 'tool') {
    return true;
  }
  return activity.kind === 'tool.failed' || activity.kind === 'tool.declined';
}

function isTurnDiffUpdatedActivity(
  activity: OrchestrationThreadActivity
): boolean {
  return activity.kind === 'turn.diff.updated';
}

function extractUnifiedDiff(
  payload: Record<string, unknown> | null
): string | undefined {
  if (payload === null || typeof payload.unifiedDiff !== 'string') {
    return undefined;
  }
  if (payload.unifiedDiff.trim().length === 0) {
    return undefined;
  }
  return payload.unifiedDiff;
}

function deriveTurnDiffEntry(
  group: ReadonlyArray<OrchestrationThreadActivity>
): WorkLogEntry | undefined {
  const latest = [...group]
    .filter(isTurnDiffUpdatedActivity)
    .sort(compareActivities)
    .at(-1);
  if (latest === undefined) {
    return undefined;
  }
  const diff = extractUnifiedDiff(asRecord(latest.payload));
  if (diff === undefined) {
    return undefined;
  }
  return {
    id: latest.id,
    kind: 'diff',
    title: 'Changes',
    status: 'done',
    diff,
  };
}

function shouldCollapse(previous: DerivingEntry, next: DerivingEntry): boolean {
  if (previous.activityKind !== 'tool.updated') {
    return false;
  }
  if (
    next.activityKind !== 'tool.updated' &&
    next.activityKind !== 'tool.completed'
  ) {
    return false;
  }
  return (
    previous.collapseKey !== undefined &&
    previous.collapseKey === next.collapseKey
  );
}

function mergeEntries(
  previous: WorkLogEntry,
  next: WorkLogEntry
): WorkLogEntry {
  return {
    ...previous,
    ...next,
    title: previous.title,
    detail: next.detail ?? previous.detail,
    diff: next.diff ?? previous.diff,
    output: next.output ?? previous.output,
  };
}

export function deriveWorkLogEntries(
  group: ReadonlyArray<OrchestrationThreadActivity>
): WorkLogEntry[] {
  const ordered = [...group].filter(isWorkToolActivity).sort(compareActivities);
  const collapsed: DerivingEntry[] = [];
  for (const activity of ordered) {
    const deriving = toDerivingEntry(activity);
    const previous = collapsed.at(-1);
    if (previous !== undefined && shouldCollapse(previous, deriving)) {
      collapsed[collapsed.length - 1] = {
        activityKind: deriving.activityKind,
        collapseKey: deriving.collapseKey ?? previous.collapseKey,
        entry: mergeEntries(previous.entry, deriving.entry),
      };
      continue;
    }
    collapsed.push(deriving);
  }
  const entries = collapsed.map((item) => item.entry);
  const diffEntry = deriveTurnDiffEntry(group);
  if (diffEntry === undefined) {
    return entries;
  }
  return [...entries, diffEntry];
}

/**
 * Present a work-log group: cap visible rows at `MAX_VISIBLE_WORK_LOG_ENTRIES`
 * and fold the overflow behind a `work-toggle` row with the hidden count.
 * `expandedGroupIds` (matched against the group's first entry id) reveals the
 * full group without a toggle.
 */
export function deriveThreadFeedPresentation(
  entries: ReadonlyArray<WorkLogEntry>,
  expandedGroupIds: ReadonlySet<string>
): ThreadFeedEntry[] {
  if (entries.length === 0) {
    return [];
  }
  const groupId = entries[0].id;
  const expanded = expandedGroupIds.has(groupId);
  const visible = expanded
    ? entries
    : entries.slice(-MAX_VISIBLE_WORK_LOG_ENTRIES);
  const result: ThreadFeedEntry[] = [];
  for (const entry of visible) {
    result.push({ kind: 'work-log', id: entry.id, entry });
  }
  // The toggle stays while expanded so the group can be folded again; its
  // count is what a collapse would hide, not what is hidden right now.
  const hiddenCount = entries.length - MAX_VISIBLE_WORK_LOG_ENTRIES;
  if (hiddenCount > 0) {
    result.push({
      kind: 'work-toggle',
      id: `work-toggle:${groupId}`,
      groupId,
      hiddenCount,
    });
  }
  return result;
}
