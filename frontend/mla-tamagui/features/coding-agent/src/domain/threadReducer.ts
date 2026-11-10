// features/coding-agent/src/domain/threadReducer.ts
//
// Pure fold of a single `OrchestrationEvent` into a `ThreadDetailState`.
// Ported from references/t3code/packages/client-runtime/src/state/threadReducer.ts
// over our `OrchestrationThread` / `OrchestrationEvent` types: no `effect`,
// no IO, total comparators on (createdAt, id).

import type {
  OrchestrationEvent,
  OrchestrationLatestTurn,
  OrchestrationSession,
  OrchestrationThread,
} from '../contracts/orchestration';

export type ThreadDetailState = OrchestrationThread & {
  turnDiffs?: Record<string, string>;
};

function compareByCreatedAtId(
  left: { createdAt: string; id: string },
  right: { createdAt: string; id: string }
): number {
  return (
    left.createdAt.localeCompare(right.createdAt) ||
    left.id.localeCompare(right.id)
  );
}

function settledTurnStateForSessionStatus(
  status: OrchestrationSession['status']
): 'completed' | 'interrupted' | 'error' | null {
  switch (status) {
    case 'idle':
    case 'ready':
      return 'completed';
    case 'error':
      return 'error';
    case 'interrupted':
    case 'stopped':
      return 'interrupted';
    case 'starting':
    case 'running':
      return null;
  }
}

/**
 * Apply a single orchestration event to a `ThreadDetailState`, returning the
 * updated thread. Event types outside our union (and no-op events) return the
 * thread unchanged.
 */
export function applyThreadDetailEvent(
  thread: ThreadDetailState,
  event: OrchestrationEvent
): ThreadDetailState {
  switch (event.type) {
    case 'thread.created': {
      const payload = event.payload;
      const created: ThreadDetailState = {
        id: event.threadId,
        title: payload.title,
        cwd: payload.cwd,
        modelSelection: payload.modelSelection,
        runtimeMode: payload.runtimeMode,
        interactionMode: payload.interactionMode,
        latestTurn: null,
        createdAt: payload.createdAt,
        updatedAt: payload.createdAt,
        archivedAt: null,
        deletedAt: null,
        messages: [],
        proposedPlans: [],
        activities: [],
        session: null,
        ...(payload.branch !== undefined ? { branch: payload.branch } : {}),
        ...(payload.worktreePath !== undefined
          ? { worktreePath: payload.worktreePath }
          : {}),
        turnDiffs: {},
      };
      return created;
    }

    case 'thread.deleted': {
      const payload = event.payload;
      return {
        ...thread,
        deletedAt: payload.deletedAt,
        updatedAt: payload.deletedAt,
      };
    }

    case 'thread.archived': {
      const payload = event.payload;
      return {
        ...thread,
        archivedAt: payload.archivedAt,
        updatedAt: payload.updatedAt,
      };
    }

    case 'thread.unarchived': {
      const payload = event.payload;
      return { ...thread, archivedAt: null, updatedAt: payload.updatedAt };
    }

    case 'thread.meta-updated': {
      const payload = event.payload;
      return {
        ...thread,
        ...(payload.title !== undefined ? { title: payload.title } : {}),
        ...(payload.cwd !== undefined ? { cwd: payload.cwd } : {}),
        ...(payload.modelSelection !== undefined
          ? { modelSelection: payload.modelSelection }
          : {}),
        ...(payload.branch !== undefined ? { branch: payload.branch } : {}),
        ...(payload.worktreePath !== undefined
          ? { worktreePath: payload.worktreePath }
          : {}),
        updatedAt: payload.updatedAt,
      };
    }

    case 'thread.runtime-mode-set': {
      const payload = event.payload;
      return {
        ...thread,
        runtimeMode: payload.runtimeMode,
        updatedAt: payload.updatedAt,
      };
    }

    case 'thread.interaction-mode-set': {
      const payload = event.payload;
      return {
        ...thread,
        interactionMode: payload.interactionMode,
        updatedAt: payload.updatedAt,
      };
    }

    case 'thread.message-sent': {
      const incoming = event.payload.message;
      const messages = thread.messages.some((entry) => entry.id === incoming.id)
        ? thread.messages.map((entry) =>
            entry.id === incoming.id ? incoming : entry
          )
        : [...thread.messages, incoming];
      return { ...thread, messages, updatedAt: event.createdAt };
    }

    case 'thread.turn-start-requested': {
      const payload = event.payload;
      return {
        ...thread,
        ...(payload.modelSelection !== undefined
          ? { modelSelection: payload.modelSelection }
          : {}),
        runtimeMode: payload.runtimeMode,
        interactionMode: payload.interactionMode,
        updatedAt: event.createdAt,
      };
    }

    case 'thread.turn-interrupt-requested': {
      const payload = event.payload;
      if (payload.turnId === undefined) return thread;
      const latestTurn = thread.latestTurn;
      if (latestTurn === null || latestTurn.turnId !== payload.turnId)
        return thread;
      return {
        ...thread,
        latestTurn: {
          ...latestTurn,
          state: 'interrupted',
          startedAt: latestTurn.startedAt ?? payload.createdAt,
          completedAt: latestTurn.completedAt ?? payload.createdAt,
        },
        updatedAt: event.createdAt,
      };
    }

    case 'thread.approval-response-requested':
    case 'thread.user-input-response-requested':
      return thread;

    case 'thread.session-stop-requested': {
      if (thread.session === null) return thread;
      return {
        ...thread,
        session: {
          ...thread.session,
          status: 'stopped',
          activeTurnId: undefined,
        },
        updatedAt: event.createdAt,
      };
    }

    case 'thread.session-set': {
      const payload = event.payload;
      const session = payload.session;
      const settledTurnState = settledTurnStateForSessionStatus(session.status);
      let latestTurn: OrchestrationLatestTurn | null = thread.latestTurn;
      if (session.status === 'running' && session.activeTurnId !== undefined) {
        latestTurn = {
          turnId: session.activeTurnId,
          state: 'running',
          requestedAt:
            thread.latestTurn?.turnId === session.activeTurnId
              ? thread.latestTurn.requestedAt
              : event.createdAt,
          startedAt:
            thread.latestTurn?.turnId === session.activeTurnId
              ? (thread.latestTurn.startedAt ?? event.createdAt)
              : event.createdAt,
          completedAt: null,
        };
      } else if (
        thread.latestTurn !== null &&
        thread.latestTurn.state === 'running' &&
        settledTurnState !== null
      ) {
        latestTurn = {
          ...thread.latestTurn,
          state: settledTurnState,
          completedAt: event.createdAt,
        };
      }
      return { ...thread, session, latestTurn, updatedAt: event.createdAt };
    }

    case 'thread.proposed-plan-upserted': {
      const plan = event.payload.proposedPlan;
      const proposedPlans = [
        ...thread.proposedPlans.filter((p) => p.id !== plan.id),
        plan,
      ].sort(compareByCreatedAtId);
      return { ...thread, proposedPlans, updatedAt: event.createdAt };
    }

    case 'thread.activity-appended': {
      const activity = event.payload.activity;
      const activities = [
        ...thread.activities.filter((a) => a.id !== activity.id),
        activity,
      ].sort(compareByCreatedAtId);
      return { ...thread, activities, updatedAt: event.createdAt };
    }

    case 'thread.turn-diff-updated': {
      const payload = event.payload;
      return {
        ...thread,
        turnDiffs: {
          ...thread.turnDiffs,
          [payload.turnId]: payload.unifiedDiff,
        },
        updatedAt: event.createdAt,
      };
    }

    default:
      return thread;
  }
}
