// features/coding-agent/src/fixtures/serverConfig.ts
//
// Deterministic `server.probe` fixture: opencode is the default CLI with two
// models, claude is reachable with two models (one flagged legacy so the
// picker can label it), codex is unreachable. Parsed against
// `ServerProbeResultSchema` at module load so the wire shape is exercised
// against the same schema the WS layer uses.

import {
  ServerProbeResultSchema,
  type ServerProbeResult,
} from '@app/remote-chat-agent-contracts/orchestration';

export const FIXTURE_SERVER_CONFIG: ServerProbeResult =
  ServerProbeResultSchema.parse({
    availableClis: ['opencode', 'claude'],
    selectedCli: 'opencode',
    version: 'fx-probe-1.0.0',
    providers: [
      {
        cli: 'opencode',
        displayName: 'OpenCode',
        available: true,
        models: [
          {
            providerID: 'opencode',
            modelID: 'gpt-5.4',
            name: 'GPT-5.4',
            isDefault: true,
          },
          {
            providerID: 'opencode',
            modelID: 'claude-sonnet-4.6',
            name: 'Claude Sonnet 4.6',
          },
        ],
      },
      {
        cli: 'claude',
        displayName: 'Claude',
        available: true,
        models: [
          {
            providerID: 'anthropic',
            modelID: 'claude-sonnet-4.6',
            name: 'Claude Sonnet 4.6',
            isDefault: true,
          },
          {
            providerID: 'anthropic',
            modelID: 'claude-3-haiku',
            name: 'Claude 3 Haiku',
          },
        ],
      },
      {
        cli: 'codex',
        displayName: 'Codex',
        available: false,
        models: [],
      },
    ],
  });
