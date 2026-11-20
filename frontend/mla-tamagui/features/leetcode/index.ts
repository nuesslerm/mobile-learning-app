/**
 * Public barrel for the LeetCode feature in app #2. Nothing outside this
 * directory may import its internals -- routes import from here only.
 *
 * A plain in-app feature: one package, no separate mocks or implementation package.
 */

// The on-device runner's provider: the app root mounts it so the interpreter outlives the screens
export { PyodideRunnerProvider } from './data/pyodide/PyodideRunnerProvider';

// Screens (ui layer)
export { ProblemListScreen } from './ui/ProblemListScreen';

// State
export { useLeetCodeAuthStore } from './state/authStore';
export { usePlanStore } from './plan';

// Ports -- pure types consumed by screens and harness doubles
export type {
  CodeRunnerPort,
  LeetCodeGatewayPort,
  LeetCodePortOverrides,
  LeetCodePorts,
  VoiceEditTransportPort,
} from './domain/ports';

// Domain types and schemas
export type { RunResponse } from './domain';
