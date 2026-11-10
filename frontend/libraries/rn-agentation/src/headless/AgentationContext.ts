/**
 * Context wiring for the headless `rn-agentation` API. The {@link AgentationProvider}
 * publishes {@link AgentationOptions} and the live {@link AgentationHandle} here;
 * {@link useAgentation} reads the handle.
 *
 * Importing this directly is unsupported — go through the public surface.
 */
import * as React from 'react';

import type { DocumentNodeService } from '../document/service/DocumentNodeService';
import type { AgentationHandle, AgentationOptions } from './types';

export const AgentationOptionsContext = React.createContext<AgentationOptions | null>(null);
export const AgentationHandleContext = React.createContext<AgentationHandle | null>(null);
export const AgentationServiceContext = React.createContext<DocumentNodeService | null>(null);

/** Read the provider's live handle. Throws if used outside an {@link AgentationProvider}. */
export function useAgentationHandle(): AgentationHandle {
  const ctx = React.useContext(AgentationHandleContext);
  if (!ctx) {
    throw new Error('useAgentation must be used within an <AgentationProvider>');
  }
  return ctx;
}
