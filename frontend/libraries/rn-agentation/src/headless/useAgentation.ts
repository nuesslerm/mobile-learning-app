/**
 * Public hook for `rn-agentation`. Returns the shared handle created by the
 * nearest {@link AgentationProvider}. Use this in the host overlay + sheet so
 * state stays synchronized across consumers.
 */
import { useAgentationHandle } from './AgentationContext';
import type { AgentationHandle } from './types';

/**
 * Read and dispatch the shared agentation state. Must be called inside an
 * `<AgentationProvider>`. The returned handle is stable for the lifetime of the
 * provider; memoize consumers that depend on individual fields via
 * `useAgentation().x` selectors.
 */
export function useAgentation(): AgentationHandle {
  return useAgentationHandle();
}
