// features/coding-agent/src/domain/threadSettings.ts
//
// Static, ordered pickers for the runtime and interaction mode rows in the
// thread settings sheet. Ported from
// `references/t3code/apps/mobile/src/features/threads/thread-settings-options.ts`
// `RUNTIME_MODE_CHOICES`: no `effect`, no IO. Plan mode is always offered so a
// disabled preference cannot silently hide the option from users on launch —
// the caller's UI hides the row when the preference is off.

import type { InteractionMode, RuntimeMode } from '../contracts/orchestration';

export interface RuntimeModeOption {
  readonly mode: RuntimeMode;
  readonly label: string;
  readonly description: string;
}

export interface InteractionModeOption {
  readonly mode: InteractionMode;
  readonly label: string;
  readonly description: string;
}

export const RUNTIME_MODE_OPTIONS: ReadonlyArray<RuntimeModeOption> = [
  {
    mode: 'approval-required',
    label: 'Supervised',
    description: 'Ask before commands and file changes.',
  },
  {
    mode: 'auto-accept-edits',
    label: 'Auto-accept edits',
    description: 'Auto-approve edits, ask before other actions.',
  },
  {
    mode: 'auto',
    label: 'Auto',
    description: 'Supported providers approve routine actions; others still ask.',
  },
  {
    mode: 'full-access',
    label: 'Full access',
    description: 'Allow commands and edits without prompts.',
  },
];

export const INTERACTION_MODE_OPTIONS: ReadonlyArray<InteractionModeOption> = [
  {
    mode: 'default',
    label: 'Build',
    description: 'Apply changes directly as the agent works.',
  },
  {
    mode: 'plan',
    label: 'Plan',
    description: 'Outline the approach first, then build after approval.',
  },
];