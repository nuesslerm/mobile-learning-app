// features/coding-agent/src/fixtures/dispatchRuns.ts
//
// Deterministic `DispatchRunDetail` fixtures driving the dispatch-runs UI
// (list summary + detail transcript) and the e2e fake server. Parsed at
// module load against the same schema the wire layer enforces.

import {
  DispatchRunDetail,
  type DispatchRunDetail as DispatchRunDetailType,
} from '../domain/dispatchRuns';

const RUN_ID_FINISHED = 'fx-run-finished';
const RUN_ID_RUNNING = 'fx-run-running';

const ISO_T0 = '2026-09-06T10:00:00.000Z';
const ISO_T2 = '2026-09-06T10:00:48.210Z';

const finishedLines: string[] = [
  JSON.stringify({
    type: 'reasoning',
    text: 'Inspect the thread list, then add the section.',
  }),
  JSON.stringify({ type: 'text', text: 'Adding the section now.' }),
  JSON.stringify({
    type: 'text',
    text: 'Done. The section sits below the live threads.',
  }),
  JSON.stringify({
    type: 'tool_use',
    name: 'read',
    input: { path: 'src/ui/ThreadListScreen.tsx' },
  }),
  JSON.stringify({
    type: 'tool_use',
    name: 'edit',
    input: {
      path: 'src/ui/ThreadListScreen.tsx',
      oldString: '<ThreadList />',
      newString: '<ThreadList />\n<DispatchRunsSection />',
    },
  }),
  JSON.stringify({
    type: 'tool_use',
    name: 'bash',
    input: { command: 'pnpm --filter mla-tamagui run typecheck' },
    output: 'Done in 12.4s using pnpm',
  }),
  JSON.stringify({ type: 'step_finish' }),
];

const runningLines: string[] = [
  JSON.stringify({
    type: 'reasoning',
    text: 'Reading the dashboard code first.',
  }),
  JSON.stringify({
    type: 'text',
    text: 'The dashboard polls every 1500ms via fetch.',
  }),
  JSON.stringify({
    type: 'tool_use',
    name: 'read',
    input: { path: 'backend/vm-ops-api/rs/static/opencode-log/index.html' },
  }),
  JSON.stringify({
    type: 'text',
    text: 'It re-renders the table on every response.',
  }),
];

const fxFinished = DispatchRunDetail.parse({
  runId: RUN_ID_FINISHED,
  prompt: 'Add the DispatchRunsSection to the thread list',
  agent: 'implementer',
  caller: null,
  claudeSessionId: null,
  model: 'minimax/MiniMax-M3',
  startedAt: ISO_T0,
  finishedAt: ISO_T2,
  status: 'finished',
  ok: true,
  tokens: { total: 12345, input: 10000, output: 2345 },
  cost: 0.0123,
  durationMs: 48210,
  lines: finishedLines,
});

const fxRunning = DispatchRunDetail.parse({
  runId: RUN_ID_RUNNING,
  prompt: 'Explain how the opencode-log dashboard polls',
  agent: 'researcher',
  caller: null,
  claudeSessionId: null,
  model: 'kimi-for-coding/k2p7',
  startedAt: ISO_T0,
  finishedAt: null,
  status: 'running',
  ok: null,
  tokens: null,
  cost: null,
  durationMs: null,
  lines: runningLines,
});

export const FIXTURE_DISPATCH_RUN_IDS = {
  finished: RUN_ID_FINISHED,
  running: RUN_ID_RUNNING,
} as const;

export const FIXTURE_DISPATCH_RUNS: {
  finished: DispatchRunDetailType;
  running: DispatchRunDetailType;
} = {
  finished: fxFinished,
  running: fxRunning,
};