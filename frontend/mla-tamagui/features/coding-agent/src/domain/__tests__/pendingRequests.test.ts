// features/coding-agent/src/domain/__tests__/pendingRequests.test.ts
//
// Tests for `derivePendingApprovals` / `derivePendingUserInputs` and the
// `sniffToolPayload` sniffer that both layers share. Every test is
// Given/When/Then; activities and threads are built with the `makeActivity`
// / `makeThread` helpers (branded ids via `.parse`).

import {
  ApprovalRequestId,
  EventId,
  ThreadId,
  TurnId,
  type OrchestrationThread,
  type OrchestrationThreadActivity,
} from '@app/remote-chat-agent-contracts/orchestration';
import { describe, expect, it } from 'vitest';

import {
  derivePendingApprovals,
  derivePendingUserInputs,
} from '../pendingRequests';
import { sniffToolPayload } from '../toolPayload';

function makeThread(
  overrides: Partial<OrchestrationThread> = {}
): OrchestrationThread {
  return {
    id: ThreadId.parse('thread-1'),
    title: 'Test Thread',
    cwd: '/repo',
    modelSelection: { cli: 'codex', modelID: 'gpt-5.4' },
    runtimeMode: 'full-access',
    interactionMode: 'default',
    latestTurn: null,
    createdAt: '2026-04-01T00:00:00.000Z',
    updatedAt: '2026-04-01T00:00:00.000Z',
    archivedAt: null,
    deletedAt: null,
    messages: [],
    proposedPlans: [],
    activities: [],
    session: null,
    ...overrides,
  };
}

function makeActivity(
  overrides: Partial<OrchestrationThreadActivity>
): OrchestrationThreadActivity {
  return {
    id: EventId.parse('activity-1'),
    tone: 'tool',
    kind: 'tool.completed',
    summary: 'Run tests',
    payload: null,
    createdAt: '2026-04-01T00:00:01.000Z',
    ...overrides,
  };
}

describe('sniffToolPayload', () => {
  it('given a command_execution payload, when sniffed, then kind=command carries the command text', () => {
    // Given a command payload with an explicit command
    const payload = {
      itemType: 'command_execution',
      command: 'pnpm test',
    };
    // When sniffed
    const result = sniffToolPayload(payload, 'Run tests');
    // Then the kind is command and both command and detail carry the value
    expect(result).toMatchObject({
      kind: 'command',
      title: 'Run tests',
      detail: 'pnpm test',
      command: 'pnpm test',
    });
  });

  it('given a file_change payload, when sniffed, then kind=file_change carries the file path', () => {
    // Given a file_change payload with changedFiles
    const payload = {
      itemType: 'file_change',
      data: { changedFiles: ['src/config.ts'] },
    };
    // When sniffed
    const result = sniffToolPayload(payload, 'Edit config');
    // Then the kind is file_change and the path surfaces in detail and path
    expect(result).toMatchObject({
      kind: 'file_change',
      title: 'Edit config',
      detail: 'src/config.ts',
      path: 'src/config.ts',
    });
  });

  it('given a web_search payload, when sniffed, then kind=web_search carries the url', () => {
    // Given a web_search payload with a url
    const payload = {
      itemType: 'web_search',
      data: { url: 'https://docs.example.com/search?q=vitest' },
    };
    // When sniffed
    const result = sniffToolPayload(payload, 'Search docs');
    // Then the kind is web_search and the url surfaces in detail and url
    expect(result).toMatchObject({
      kind: 'web_search',
      title: 'Search docs',
      detail: 'https://docs.example.com/search?q=vitest',
      url: 'https://docs.example.com/search?q=vitest',
    });
  });

  it('given an mcp payload, when sniffed, then kind=mcp carries the server.tool identity', () => {
    // Given an mcp_tool_call payload with a server/tool
    const payload = {
      itemType: 'mcp_tool_call',
      data: { item: { server: 'repository', tool: 'search' } },
    };
    // When sniffed
    const result = sniffToolPayload(payload, 'Call repository tool');
    // Then the kind is mcp and the identity surfaces as server.tool
    expect(result).toMatchObject({
      kind: 'mcp',
      title: 'Call repository tool',
      detail: 'repository.search',
      command: 'repository.search',
    });
  });
});

describe('derivePendingApprovals', () => {
  it('given an open and a resolved approval, when derived, then only the open approval is pending', () => {
    // Given a thread with two request.opened, one with a matching request.resolved
    const openRequestId = ApprovalRequestId.parse('req-open');
    const resolvedRequestId = ApprovalRequestId.parse('req-resolved');
    const thread = makeThread({
      activities: [
        makeActivity({
          id: EventId.parse('activity-open'),
          tone: 'approval',
          kind: 'request.opened',
          summary: 'Approve command',
          payload: {
            requestId: openRequestId,
            requestType: 'command_execution_approval',
            detail: 'pnpm test',
          },
          createdAt: '2026-04-01T00:00:01.000Z',
        }),
        makeActivity({
          id: EventId.parse('activity-resolved'),
          tone: 'approval',
          kind: 'request.opened',
          summary: 'Approve patch',
          payload: {
            requestId: resolvedRequestId,
            requestType: 'file_change_approval',
            detail: 'src/foo.ts',
          },
          createdAt: '2026-04-01T00:00:02.000Z',
        }),
        makeActivity({
          id: EventId.parse('activity-resolve'),
          tone: 'approval',
          kind: 'request.resolved',
          summary: 'Resolved patch',
          payload: {
            requestId: resolvedRequestId,
            decision: 'accept',
          },
          createdAt: '2026-04-01T00:00:03.000Z',
        }),
      ],
    });
    // When derived
    const pending = derivePendingApprovals(thread);
    // Then only the open approval remains
    expect(pending).toHaveLength(1);
    expect(pending[0]).toMatchObject({
      requestId: openRequestId,
      kind: 'command',
      title: 'Approve command',
      detail: 'pnpm test',
    });
  });

  it('given an open approval with args, when derived, then the args are carried through', () => {
    // Given a request.opened carrying args
    const requestId = ApprovalRequestId.parse('req-1');
    const args = { command: 'rm -rf /', cwd: '/etc' };
    const turnId = TurnId.parse('turn-1');
    const thread = makeThread({
      activities: [
        makeActivity({
          id: EventId.parse('activity-open'),
          tone: 'approval',
          kind: 'request.opened',
          summary: 'Approve command',
          payload: {
            requestId,
            requestType: 'exec_command_approval',
            detail: 'rm -rf /',
            args,
          },
          createdAt: '2026-04-01T00:00:01.000Z',
          turnId,
        }),
      ],
    });
    // When derived
    const pending = derivePendingApprovals(thread);
    // Then the args and turnId flow through
    expect(pending).toHaveLength(1);
    expect(pending[0]).toMatchObject({
      requestId,
      kind: 'command',
      title: 'Approve command',
      detail: 'rm -rf /',
      args,
      turnId,
    });
  });

  it('given no approval activities, when derived, then the result is empty', () => {
    // Given an empty thread
    const thread = makeThread();
    // When derived
    const pending = derivePendingApprovals(thread);
    // Then there are no pending approvals
    expect(pending).toEqual([]);
  });
});

describe('derivePendingUserInputs', () => {
  it('given a user-input request with two questions, when derived, then both questions are carried', () => {
    // Given a user-input.requested with two questions
    const requestId = ApprovalRequestId.parse('req-1');
    const turnId = TurnId.parse('turn-1');
    const questions = [
      {
        id: 'q1',
        header: 'Auth',
        question: 'Pick one',
        options: [
          { label: 'Option A', description: 'desc a' },
          { label: 'Option B', description: 'desc b' },
        ],
        multiSelect: false,
      },
      {
        id: 'q2',
        header: 'Scope',
        question: 'Pick all that apply',
        options: [
          { label: 'Option C', description: 'desc c' },
          { label: 'Option D', description: 'desc d' },
        ],
        multiSelect: true,
      },
    ];
    const thread = makeThread({
      activities: [
        makeActivity({
          id: EventId.parse('activity-input'),
          tone: 'approval',
          kind: 'user-input.requested',
          summary: 'Need input',
          payload: { requestId, questions },
          createdAt: '2026-04-01T00:00:01.000Z',
          turnId,
        }),
      ],
    });
    // When derived
    const pending = derivePendingUserInputs(thread);
    // Then both questions are present and turnId flows through
    expect(pending).toHaveLength(1);
    expect(pending[0]).toMatchObject({
      requestId,
      turnId,
    });
    expect(pending[0]?.questions).toHaveLength(2);
    expect(pending[0]?.questions[0]).toMatchObject({ id: 'q1' });
    expect(pending[0]?.questions[1]).toMatchObject({ id: 'q2', multiSelect: true });
  });

  it('given an open and a resolved user-input request, when derived, then only the open one is pending', () => {
    // Given a user-input.requested with a matching user-input.resolved and a second open one
    const resolvedRequestId = ApprovalRequestId.parse('req-resolved');
    const openRequestId = ApprovalRequestId.parse('req-open');
    const questions = [
      {
        id: 'q1',
        header: 'Auth',
        question: 'Pick one',
        options: [{ label: 'Option A', description: 'desc a' }],
        multiSelect: false,
      },
    ];
    const thread = makeThread({
      activities: [
        makeActivity({
          id: EventId.parse('activity-input-resolved'),
          tone: 'approval',
          kind: 'user-input.requested',
          summary: 'Need input',
          payload: { requestId: resolvedRequestId, questions },
          createdAt: '2026-04-01T00:00:01.000Z',
        }),
        makeActivity({
          id: EventId.parse('activity-resolve'),
          tone: 'approval',
          kind: 'user-input.resolved',
          summary: 'Resolved',
          payload: { requestId: resolvedRequestId, answers: { q1: 'Option A' } },
          createdAt: '2026-04-01T00:00:02.000Z',
        }),
        makeActivity({
          id: EventId.parse('activity-input-open'),
          tone: 'approval',
          kind: 'user-input.requested',
          summary: 'Need input again',
          payload: { requestId: openRequestId, questions },
          createdAt: '2026-04-01T00:00:03.000Z',
        }),
      ],
    });
    // When derived
    const pending = derivePendingUserInputs(thread);
    // Then only the open request remains
    expect(pending).toHaveLength(1);
    expect(pending[0]).toMatchObject({ requestId: openRequestId });
  });
});
