// features/coding-agent/src/domain/pendingRequests.ts
//
// Pure derivation of the still-open approval and user-input requests for a
// thread. Ported from `references/t3code/apps/mobile/src/lib/threadActivity.ts`
// `derivePendingApprovals` / `derivePendingUserInputs` over our
// `OrchestrationThread` shapes: no `effect`, no IO, total comparators on
// (createdAt, sequence, id). An approval is `request.opened` without a
// matching `request.resolved`; a user-input is `user-input.requested` without
// a matching `user-input.resolved`.

import {
  ApprovalRequestId,
  type OrchestrationThread,
  type OrchestrationThreadActivity,
  type TurnId,
  type UserInputQuestion,
} from '@app/remote-chat-agent-contracts/orchestration';

import { type ToolPayloadKind } from './toolPayload';

export interface PendingApproval {
  requestId: ApprovalRequestId;
  kind: ToolPayloadKind;
  title: string;
  detail?: string;
  args?: unknown;
  turnId?: TurnId;
}

export interface PendingUserInput {
  requestId: ApprovalRequestId;
  questions: ReadonlyArray<UserInputQuestion>;
  turnId?: TurnId;
}

interface RequestOpenedPayload {
  requestType?: unknown;
  detail?: unknown;
  args?: unknown;
}

interface UserInputRequestedPayload {
  questions?: ReadonlyArray<UserInputQuestion>;
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

function parseApprovalRequestId(value: unknown): ApprovalRequestId | null {
  return typeof value === 'string' && value.length > 0
    ? ApprovalRequestId.parse(value)
    : null;
}

function requestKindFromRequestType(
  requestType: unknown
): ToolPayloadKind | null {
  switch (requestType) {
    case 'command_execution_approval':
    case 'exec_command_approval':
      return 'command';
    case 'file_read_approval':
      return 'file_read';
    case 'file_change_approval':
    case 'apply_patch_approval':
      return 'file_change';
    default:
      return null;
  }
}

function approvalKind(
  payload: RequestOpenedPayload | null
): ToolPayloadKind {
  const fromRequestType = requestKindFromRequestType(payload?.requestType);
  if (fromRequestType !== null) {
    return fromRequestType;
  }
  return 'tool';
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

/**
 * Walk the thread's activities in lifecycle order and return every approval
 * whose `request.opened` has no matching `request.resolved`. Map insertion
 * order matches lifecycle order, so the result is already a stable queue.
 */
export function derivePendingApprovals(
  thread: OrchestrationThread
): PendingApproval[] {
  const open = new Map<ApprovalRequestId, PendingApproval>();
  const ordered = [...thread.activities].sort(compareActivities);

  for (const activity of ordered) {
    const payload = asRecord(activity.payload);
    const requestId = parseApprovalRequestId(payload?.requestId);

    if (activity.kind === 'request.opened' && requestId !== null) {
      const detail = asTrimmedString(payload?.detail) ?? undefined;
      const opened: PendingApproval = {
        requestId,
        kind: approvalKind(payload as RequestOpenedPayload | null),
        title: activity.summary,
        ...(detail !== null ? { detail } : {}),
        ...(payload !== null && 'args' in payload
          ? { args: (payload as RequestOpenedPayload).args }
          : {}),
        ...(activity.turnId !== undefined ? { turnId: activity.turnId } : {}),
      };
      open.set(requestId, opened);
      continue;
    }

    if (activity.kind === 'request.resolved' && requestId !== null) {
      open.delete(requestId);
    }
  }

  return [...open.values()];
}

/**
 * Walk the thread's activities in lifecycle order and return every
 * `user-input.requested` whose `user-input.resolved` has not arrived. Map
 * insertion order matches lifecycle order, so the result is already a
 * stable queue.
 */
export function derivePendingUserInputs(
  thread: OrchestrationThread
): PendingUserInput[] {
  const open = new Map<ApprovalRequestId, PendingUserInput>();
  const ordered = [...thread.activities].sort(compareActivities);

  for (const activity of ordered) {
    const payload = asRecord(activity.payload);
    const requestId = parseApprovalRequestId(payload?.requestId);

    if (activity.kind === 'user-input.requested' && requestId !== null) {
      const questions =
        (payload as UserInputRequestedPayload | null)?.questions ?? [];
      open.set(requestId, {
        requestId,
        questions,
        ...(activity.turnId !== undefined ? { turnId: activity.turnId } : {}),
      });
      continue;
    }

    if (activity.kind === 'user-input.resolved' && requestId !== null) {
      open.delete(requestId);
    }
  }

  return [...open.values()];
}
