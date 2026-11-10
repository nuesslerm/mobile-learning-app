// features/coding-agent/src/domain/__tests__/workLog.test.ts
//
// Ported in structure from
// references/t3code/apps/mobile/src/lib/threadActivity.test.ts onto our
// `OrchestrationThread` shapes and `WorkLogEntry` / `ThreadFeedEntry` unions.
// Every test is Given/When/Then; activities are built with the `makeActivity`
// helper (branded ids via `.parse`).

import {
  EventId,
  TurnId,
  type OrchestrationThreadActivity,
} from '@app/remote-chat-agent-contracts/orchestration';
import { describe, expect, it } from 'vitest';

import {
  MAX_VISIBLE_WORK_LOG_ENTRIES,
  deriveThreadFeedPresentation,
  deriveWorkLogEntries,
} from '../workLog';

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

describe('deriveWorkLogEntries', () => {
  it('given a tool activity with a command payload, when derived, then a command entry with the command detail is returned', () => {
    // Given a command_execution tool activity
    const activity = makeActivity({
      id: EventId.parse('command-1'),
      summary: 'Run tests',
      payload: {
        itemType: 'command_execution',
        command: 'bun run test',
        status: 'completed',
      },
    });
    // When derived
    const entries = deriveWorkLogEntries([activity]);
    // Then one command entry with the command as detail and a done status
    expect(entries).toEqual([
      {
        id: 'command-1',
        kind: 'command',
        title: 'Run tests',
        detail: 'bun run test',
        status: 'done',
      },
    ]);
  });

  it('given a file-change tool activity, when derived, then a file_change entry with the changed file is returned', () => {
    // Given a file_change tool activity carrying a patch
    const activity = makeActivity({
      id: EventId.parse('file-1'),
      summary: 'Edit config',
      payload: {
        itemType: 'file_change',
        status: 'completed',
        data: {
          changedFiles: ['src/config.ts'],
          patch: '--- a/src/config.ts\n+++ b/src/config.ts\n',
        },
      },
    });
    // When derived
    const entries = deriveWorkLogEntries([activity]);
    // Then a file_change entry with the file as detail and the patch as diff
    expect(entries).toEqual([
      {
        id: 'file-1',
        kind: 'file_change',
        title: 'Edit config',
        detail: 'src/config.ts',
        status: 'done',
        diff: '--- a/src/config.ts\n+++ b/src/config.ts\n',
      },
    ]);
  });

  it('given a web-search tool activity with a url, when derived, then a web_search entry is returned', () => {
    // Given a web_search tool activity
    const activity = makeActivity({
      id: EventId.parse('search-1'),
      summary: 'Search docs',
      payload: {
        itemType: 'web_search',
        status: 'completed',
        data: { url: 'https://docs.example.com/search?q=vitest' },
      },
    });
    // When derived
    const entries = deriveWorkLogEntries([activity]);
    // Then a web_search entry with the url as detail
    expect(entries).toEqual([
      {
        id: 'search-1',
        kind: 'web_search',
        title: 'Search docs',
        detail: 'https://docs.example.com/search?q=vitest',
        status: 'done',
      },
    ]);
  });

  it('given an mcp tool activity, when derived, then an mcp entry with server.tool as detail is returned', () => {
    // Given an mcp_tool_call activity
    const activity = makeActivity({
      id: EventId.parse('mcp-1'),
      summary: 'Call repository tool',
      payload: {
        title: 'Call repository tool',
        itemType: 'mcp_tool_call',
        detail: 'repository.search',
        status: 'completed',
        data: { item: { server: 'repository', tool: 'search' } },
      },
    });
    // When derived
    const entries = deriveWorkLogEntries([activity]);
    // Then an mcp entry identifying the server and tool
    expect(entries).toEqual([
      {
        id: 'mcp-1',
        kind: 'mcp',
        title: 'Call repository tool',
        detail: 'repository.search',
        status: 'done',
      },
    ]);
  });

  it('given a failed tool activity, when derived, then a failed entry is returned', () => {
    // Given a failed command activity
    const activity = makeActivity({
      id: EventId.parse('command-failed'),
      tone: 'error',
      kind: 'tool.failed',
      summary: 'Run command',
      payload: {
        itemType: 'command_execution',
        command: 'nope',
        status: 'failed',
      },
    });
    // When derived
    const entries = deriveWorkLogEntries([activity]);
    // Then the entry carries a failed status
    expect(entries).toEqual([
      {
        id: 'command-failed',
        kind: 'command',
        title: 'Run command',
        detail: 'nope',
        status: 'failed',
      },
    ]);
  });

  it('given a declined tool activity, when derived, then a declined entry is returned', () => {
    // Given a declined command activity
    const activity = makeActivity({
      id: EventId.parse('command-declined'),
      tone: 'error',
      kind: 'tool.declined',
      summary: 'Delete prod db',
      payload: {
        itemType: 'command_execution',
        command: 'rm -rf prod',
        status: 'declined',
      },
    });
    // When derived
    const entries = deriveWorkLogEntries([activity]);
    // Then the entry carries a declined status
    expect(entries).toEqual([
      {
        id: 'command-declined',
        kind: 'command',
        title: 'Delete prod db',
        detail: 'rm -rf prod',
        status: 'declined',
      },
    ]);
  });

  it('given a tool.updated and tool.completed sharing a command, when derived, then they collapse into one entry', () => {
    // Given lifecycle rows for the same command
    const turnId = TurnId.parse('turn-1');
    const activities = [
      makeActivity({
        id: EventId.parse('tool-updated'),
        kind: 'tool.updated',
        tone: 'tool',
        summary: 'Run tests',
        payload: {
          title: 'Run tests',
          itemType: 'command_execution',
          detail: "/bin/zsh -lc 'bun run test'",
          status: 'inProgress',
        },
        createdAt: '2026-04-01T00:00:01.000Z',
        turnId,
      }),
      makeActivity({
        id: EventId.parse('tool-completed'),
        kind: 'tool.completed',
        tone: 'tool',
        summary: 'Run tests completed',
        payload: {
          title: 'Run tests',
          itemType: 'command_execution',
          detail: "/bin/zsh -lc 'bun run test'",
          status: 'completed',
        },
        createdAt: '2026-04-01T00:00:02.000Z',
        turnId,
      }),
    ];
    // When derived
    const entries = deriveWorkLogEntries(activities);
    // Then a single collapsed entry with the terminal id and status
    expect(entries).toEqual([
      {
        id: 'tool-completed',
        kind: 'command',
        title: 'Run tests',
        detail: "/bin/zsh -lc 'bun run test'",
        status: 'done',
      },
    ]);
  });

  it('given a non-tool activity, when derived, then no work-log entry is returned', () => {
    // Given an info-toned activity
    const activity = makeActivity({
      id: EventId.parse('info-1'),
      tone: 'info',
      kind: 'runtime.warning',
      summary: 'Runtime warning',
    });
    // When derived
    const entries = deriveWorkLogEntries([activity]);
    // Then it is not treated as work
    expect(entries).toEqual([]);
  });
});

describe('deriveThreadFeedPresentation', () => {
  it('given 8 tool activities with one failed and one declined, when presented collapsed, then one visible entry and a toggle with hiddenCount 7', () => {
    // Given 6 command + 2 file_change tool activities (one failed, one declined)
    const turnId = TurnId.parse('turn-1');
    const commands = [
      'pnpm typecheck',
      'pnpm test',
      'git add -A',
      'git commit -m "feat: work log"',
      'wf lint',
      'wf build',
    ];
    const activities: OrchestrationThreadActivity[] = commands.map(
      (command, index) =>
        makeActivity({
          id: EventId.parse(`command-${index + 1}`),
          kind:
            index === 4
              ? 'tool.failed'
              : index === 5
                ? 'tool.declined'
                : 'tool.completed',
          tone: index === 4 || index === 5 ? 'error' : 'tool',
          summary: command,
          payload: {
            itemType: 'command_execution',
            command,
            status:
              index === 4 ? 'failed' : index === 5 ? 'declined' : 'completed',
          },
          createdAt: `2026-04-01T00:00:0${index + 1}.000Z`,
          turnId,
        })
    );
    const fileActivities: OrchestrationThreadActivity[] = [1, 2].map(
      (fileIndex) =>
        makeActivity({
          id: EventId.parse(`file-${fileIndex}`),
          kind: 'tool.completed',
          tone: 'tool',
          summary: `Edit src/foo-${fileIndex}.ts`,
          payload: {
            itemType: 'file_change',
            status: 'completed',
            data: { changedFiles: [`src/foo-${fileIndex}.ts`] },
          },
          createdAt: `2026-04-01T00:00:0${6 + fileIndex}.000Z`,
          turnId,
        })
    );
    const allActivities = [...activities, ...fileActivities];
    // When derived and presented without expansion
    const entries = deriveWorkLogEntries(allActivities);
    const presented = deriveThreadFeedPresentation(entries, new Set());
    // Then one visible entry and a toggle with hiddenCount 7
    expect(MAX_VISIBLE_WORK_LOG_ENTRIES).toBe(1);
    expect(entries).toHaveLength(8);
    expect(presented.filter((entry) => entry.kind === 'work-log')).toHaveLength(
      1
    );
    const toggle = presented.find((entry) => entry.kind === 'work-toggle');
    expect(toggle).toMatchObject({
      kind: 'work-toggle',
      hiddenCount: 7,
    });
  });

  it('given a work-log group whose id is expanded, when presented, then all entries are visible and the toggle stays to fold them again', () => {
    // Given 3 distinct tool entries sharing a group id
    const activities = [1, 2, 3].map((index) =>
      makeActivity({
        id: EventId.parse(`tool-${index}`),
        summary: `Tool ${index}`,
        payload: {
          itemType: 'command_execution',
          command: `run tool ${index}`,
          status: 'completed',
        },
        createdAt: `2026-04-01T00:00:0${index}.000Z`,
      })
    );
    const entries = deriveWorkLogEntries(activities);
    const groupId = entries[0].id;
    // When presented with the group expanded
    const presented = deriveThreadFeedPresentation(entries, new Set([groupId]));
    // Then all three work-log rows are visible and the toggle still offers to fold two of them
    expect(presented.filter((entry) => entry.kind === 'work-log')).toHaveLength(
      3
    );
    const toggle = presented.find((entry) => entry.kind === 'work-toggle');
    expect(toggle).toMatchObject({ kind: 'work-toggle', groupId, hiddenCount: 2 });
  });

  it('given an empty group, when presented, then no rows are returned', () => {
    // Given no entries
    // When presented
    const presented = deriveThreadFeedPresentation([], new Set());
    // Then the presentation is empty
    expect(presented).toEqual([]);
  });
});
