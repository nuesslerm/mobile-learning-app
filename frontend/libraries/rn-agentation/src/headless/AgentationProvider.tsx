/**
 * Headless provider for `rn-agentation`. Wraps the React tree in contexts that
 * {@link useAgentation} reads; renders no UI itself. The host app is responsible
 * for mounting its own FAB + overlay using the returned handle.
 */
import * as React from 'react';
import { Platform } from 'react-native';

import { createNativeDocumentTreeDriver } from '../document/driver/documentTree.native';
import { createWebDocumentTreeDriver } from '../document/driver/documentTree.web';
import { DocumentNodeService } from '../document/service/DocumentNodeService';
import {
  AgentationHandleContext,
  AgentationOptionsContext,
  AgentationServiceContext,
} from './AgentationContext';
import { useAgentationState } from './useAgentationState';
import type { AgentationOptions } from './types';

const PLATFORM: 'web' | 'native' = Platform.OS === 'web' ? 'web' : 'native';

export interface AgentationProviderProps {
  children: React.ReactNode;
  /** See {@link AgentationOptions}. Unspecified fields fall back to safe defaults. */
  options?: AgentationOptions;
}

export function AgentationProvider(props: AgentationProviderProps): React.ReactElement {
  const { children, options } = props;
  const resolvedOptions = React.useMemo<AgentationOptions>(
    () => ({ enabled: true, ...options }),
    // Re-create when the host swaps options (rare). Consumers don't depend on identity.
    [options],
  );

  const serviceRef = React.useRef<DocumentNodeService | null>(null);
  if (!serviceRef.current) {
    const driver =
      PLATFORM === 'web'
        ? createWebDocumentTreeDriver({
            logger: resolvedOptions.logger,
            ignoredSelectors: resolvedOptions.documentTreeDriverOptions?.ignoredSelectors,
          })
        : createNativeDocumentTreeDriver({
            logger: resolvedOptions.logger,
            inspectedView: resolvedOptions.inspectedRef?.current ?? undefined,
          });
    serviceRef.current = new DocumentNodeService(driver, resolvedOptions.logger);
  }

  const handle = useAgentationState(resolvedOptions, serviceRef.current);

  return (
    <AgentationOptionsContext.Provider value={resolvedOptions}>
      <AgentationServiceContext.Provider value={serviceRef.current}>
        <AgentationHandleContext.Provider value={handle}>{children}</AgentationHandleContext.Provider>
      </AgentationServiceContext.Provider>
    </AgentationOptionsContext.Provider>
  );
}
