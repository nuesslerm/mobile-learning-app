// features/coding-agent/src/domain/toolPayload.ts
//
// Pure, total sniffer for an `OrchestrationThreadActivity.payload` record:
// returns the canonical `{kind, title, detail?, command?, path?, url?,
// diff?, output?}` shape used by `workLog.ts` (to label a work-log row) and
// `pendingRequests.ts` (to label an open approval). Ported from the sniffing
// helpers around `references/t3code/apps/mobile/src/lib/threadActivity.ts`
// ~347-440 onto our `OrchestrationThread` shapes: no `effect`, no IO.

export type ToolPayloadKind =
  | 'command'
  | 'file_change'
  | 'file_read'
  | 'web_search'
  | 'mcp'
  | 'tool';

export interface ToolPayload {
  kind: ToolPayloadKind;
  title: string;
  detail?: string;
  command?: string;
  path?: string;
  url?: string;
  diff?: string;
  output?: string;
}

interface McpIdentity {
  server: string;
  tool: string;
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

function sniffToolPayloadKind(
  payload: Record<string, unknown> | null,
  command: string | null,
  files: readonly string[],
  mcp: McpIdentity | null,
  url: string | null
): ToolPayloadKind {
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

function sniffTitle(
  payload: Record<string, unknown> | null,
  fallback: string
): string {
  return asTrimmedString(payload?.title) ?? fallback;
}

function sniffDetail(
  kind: ToolPayloadKind,
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

function sniffOutput(payload: Record<string, unknown> | null): string | undefined {
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

function sniffDiff(payload: Record<string, unknown> | null): string | undefined {
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

/**
 * Sniff a tool-payload record into the canonical `{kind, title, detail?,
 * command?, path?, url?, diff?, output?}` shape. `fallbackTitle` is used when
 * the payload carries no explicit title (typical: the activity's `summary`).
 */
export function sniffToolPayload(
  payload: unknown,
  fallbackTitle = ''
): ToolPayload {
  const record = asRecord(payload);
  const command = extractCommand(record);
  const files = extractChangedFiles(record);
  const mcp = sniffMcpIdentity(record);
  const url = sniffUrl(record);
  const kind = sniffToolPayloadKind(record, command, files, mcp, url);
  const detail = sniffDetail(kind, record, command, files, mcp, url);
  const diff = sniffDiff(record);
  const output = sniffOutput(record);
  const title = sniffTitle(record, fallbackTitle);
  const result: ToolPayload = { kind, title };
  if (detail !== undefined) result.detail = detail;
  if (kind === 'command' && command !== null) result.command = command;
  if (
    (kind === 'file_change' || kind === 'file_read') &&
    files.length > 0
  ) {
    result.path = files[0];
  }
  if (kind === 'web_search' && url !== null) result.url = url;
  if (kind === 'mcp' && mcp !== null) {
    const identity =
      mcp.server.length > 0 && mcp.tool.length > 0
        ? `${mcp.server}.${mcp.tool}`
        : mcp.server.length > 0
          ? mcp.server
          : mcp.tool;
    result.command = identity;
  }
  if (diff !== undefined) result.diff = diff;
  if (output !== undefined) result.output = output;
  return result;
}
