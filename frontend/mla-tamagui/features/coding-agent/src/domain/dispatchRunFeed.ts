// features/coding-agent/src/domain/dispatchRunFeed.ts
//
// Build the flattened `ThreadFeed` view for one dispatch run: a single user
// prompt, the coalesced assistant text, one `work-log` row per tool_use, and
// the trailing working/error markers that drive the live transcript.

import {
  EventId,
  MessageId,
  TurnId,
  type OrchestrationMessage,
  type OrchestrationThreadActivity,
} from '@app/remote-chat-agent-contracts/orchestration';

import { type DispatchRunDetail, type DispatchRunLine, parseDispatchRunLine } from '../contracts/dispatchRuns';
import type { ThreadFeedEntry } from './threadFeed';
import type { WorkLogEntry, WorkLogKind } from './workLog';
import { sniffToolPayload } from './toolPayload';

export interface DispatchRunFeed {
  feed: ThreadFeedEntry[];
  reasoningByTurnId: Record<string, string>;
  streamingTurnId?: string;
}

const REASONING_TEXT_LIMIT = 200;
const TOOL_DETAIL_LIMIT = 200;

function stringifiedOutput(line: DispatchRunLine): string {
  if (line.type !== 'tool_use') {
    return '';
  }
  if (line.output === undefined || line.output === null) {
    return '';
  }
  if (typeof line.output === 'string') {
    return line.output;
  }
  return JSON.stringify(line.output);
}

function toolStatusFromOutput(output: string): 'done' | 'failed' {
  return output.toLowerCase().includes('error') ? 'failed' : 'done';
}

function toolKindFromName(name: string, input: unknown): WorkLogKind {
  const payload =
    input !== null && typeof input === 'object' && !Array.isArray(input)
      ? { title: name, ...(input as Record<string, unknown>) }
      : { title: name };
  return sniffToolPayload(payload, name).kind;
}

function toolDetailFromInput(input: unknown): string | undefined {
  if (input === undefined) {
    return undefined;
  }
  const serialized = JSON.stringify(input);
  return serialized.length > TOOL_DETAIL_LIMIT
    ? serialized.slice(0, TOOL_DETAIL_LIMIT)
    : serialized;
}

function buildWorkLogEntry(
  runId: string,
  toolIndex: number,
  line: Extract<DispatchRunLine, { type: 'tool_use' }>
): WorkLogEntry {
  const output = stringifiedOutput(line);
  const entry: WorkLogEntry = {
    id: `run:${runId}:tool:${toolIndex}`,
    kind: toolKindFromName(line.name, line.input),
    title: line.name,
    status: toolStatusFromOutput(output),
  };
  const detail = toolDetailFromInput(line.input);
  if (detail !== undefined) {
    entry.detail = detail;
  }
  if (output.length > 0) {
    entry.output = output;
  }
  return entry;
}

function buildErrorActivity(runId: string, startedAt: string): OrchestrationThreadActivity {
  return {
    id: `run:${runId}:error` as EventId,
    tone: 'error',
    kind: 'dispatch.failed',
    summary: 'Run failed',
    payload: null,
    turnId: `run:${runId}` as TurnId,
    createdAt: startedAt,
  };
}

export function buildDispatchRunFeed(detail: DispatchRunDetail): DispatchRunFeed {
  const turnId = `run:${detail.runId}`;
  const feed: ThreadFeedEntry[] = [];
  const reasoningByTurnId: Record<string, string> = {};
  const textSegments: string[] = [];
  const reasoningSegments: string[] = [];
  let toolIndex = 0;
  let hasErrorLine = false;

  const userMessage: OrchestrationMessage = {
    id: `run:${detail.runId}:msg:0` as MessageId,
    role: 'user',
    text: detail.prompt,
    turnId: turnId as TurnId,
    createdAt: detail.startedAt,
    updatedAt: detail.startedAt,
  };
  feed.push({
    kind: 'user-message',
    id: userMessage.id,
    turnId,
    message: userMessage,
  });

  for (const raw of detail.lines) {
    const line = parseDispatchRunLine(raw);
    switch (line.type) {
      case 'text':
        textSegments.push(line.text);
        break;
      case 'reasoning':
        reasoningSegments.push(line.text);
        break;
      case 'tool_use': {
        toolIndex += 1;
        const workEntry = buildWorkLogEntry(detail.runId, toolIndex, line);
        feed.push({
          kind: 'work-log',
          id: workEntry.id,
          turnId,
          entry: workEntry,
        });
        break;
      }
      case 'error':
        hasErrorLine = true;
        break;
      case 'step_start':
      case 'step_finish':
      case 'unknown':
        break;
    }
  }

  if (textSegments.length > 0) {
    const assistantMessage: OrchestrationMessage = {
      id: `run:${detail.runId}:msg:1` as MessageId,
      role: 'assistant',
      text: textSegments.join('\n'),
      turnId: turnId as TurnId,
      createdAt: detail.startedAt,
      updatedAt: detail.startedAt,
    };
    feed.push({
      kind: 'assistant-message',
      id: assistantMessage.id,
      turnId,
      message: assistantMessage,
    });
  }

  if (reasoningSegments.length > 0) {
    reasoningByTurnId[turnId] = reasoningSegments.join('\n').slice(0, REASONING_TEXT_LIMIT);
  }

  if (hasErrorLine) {
    const errorActivity = buildErrorActivity(detail.runId, detail.startedAt);
    feed.push({
      kind: 'error',
      id: errorActivity.id,
      turnId,
      activity: errorActivity,
    });
  }

  let streamingTurnId: string | undefined;
  if (detail.status === 'running') {
    feed.push({
      kind: 'working',
      id: `run:${detail.runId}:working`,
      turnId,
    });
    streamingTurnId = turnId;
  } else if (detail.ok === false) {
    const errorActivity = buildErrorActivity(detail.runId, detail.startedAt);
    feed.push({
      kind: 'error',
      id: errorActivity.id,
      turnId,
      activity: errorActivity,
    });
  }

  return {
    feed,
    reasoningByTurnId,
    ...(streamingTurnId !== undefined ? { streamingTurnId } : {}),
  };
}