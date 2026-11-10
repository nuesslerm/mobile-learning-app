import * as React from 'react';
import { Platform } from 'react-native';

import { noopLogger } from '../../domain/types';
import { createNativeDocumentTreeDriver } from '../driver/documentTree.native';
import { createWebDocumentTreeDriver } from '../driver/documentTree.web';
import { DocumentNodeService } from '../service/DocumentNodeService';
import type { DocumentNode, DocumentTreeDriver, UseDocumentNodeServiceOptions } from '../domain/types';

const PLATFORM: 'web' | 'native' = Platform.OS === 'web' ? 'web' : 'native';

function createDefaultDriver(): DocumentTreeDriver {
  return PLATFORM === 'web'
    ? createWebDocumentTreeDriver()
    : createNativeDocumentTreeDriver();
}

export interface UseDocumentNodeServiceResult {
  snapshot: DocumentNode | null;
  isLoading: boolean;
  error: Error | null;
  queryAtPoint: (x: number, y: number) => Promise<DocumentNode | undefined>;
  queryByName: (name: string) => DocumentNode[];
  queryBySource: (file: string) => DocumentNode[];
  queryByTestId: (testId: string) => DocumentNode[];
  query: (predicate: (node: DocumentNode) => boolean) => DocumentNode[];
  refresh: () => Promise<void>;
  selectedNode: DocumentNode | undefined;
  selectNode: (id: string) => void;
}

export function useDocumentNodeService(options?: UseDocumentNodeServiceOptions): UseDocumentNodeServiceResult {
  const logger = options?.logger ?? noopLogger;
  const [snapshot, setSnapshot] = React.useState<DocumentNode | null>(null);
  const [isLoading, setIsLoading] = React.useState(false);
  const [error, setError] = React.useState<Error | null>(null);
  const [selectedId, setSelectedId] = React.useState<string | null>(null);

  const serviceRef = React.useRef<DocumentNodeService | null>(null);
  if (!serviceRef.current) {
    const driver = options?.driver ?? createDefaultDriver();
    serviceRef.current = new DocumentNodeService(driver, logger);
  }
  const service = serviceRef.current;

  const refresh = React.useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      await service.refresh();
    } catch (err) {
      setError(err instanceof Error ? err : new Error(String(err)));
    } finally {
      setIsLoading(false);
    }
  }, [service]);

  const selectNode = React.useCallback((id: string) => {
    service.selectNode(id);
  }, [service]);

  React.useEffect(() => {
    void refresh();
    return service.subscribe(() => {
      setSnapshot(service.currentSnapshot);
      setSelectedId(service.currentSelectedId);
    });
  }, [service, refresh]);

  const selectedNode = React.useMemo(() => {
    if (!snapshot || !selectedId) return undefined;
    return service.queryById(selectedId) ?? undefined;
  }, [service, snapshot, selectedId]);

  const queryAtPoint = React.useCallback(
    async (x: number, y: number) => service.queryAtPoint(x, y),
    [service],
  );
  const queryByName = React.useCallback(
    (name: string) => service.queryByName(name),
    [service],
  );
  const queryBySource = React.useCallback(
    (file: string) => service.queryBySource(file),
    [service],
  );
  const queryByTestId = React.useCallback(
    (testId: string) => service.queryByTestId(testId),
    [service],
  );
  const query = React.useCallback(
    (predicate: (node: DocumentNode) => boolean) => service.query(predicate),
    [service],
  );

  return {
    snapshot,
    isLoading,
    error,
    queryAtPoint,
    queryByName,
    queryBySource,
    queryByTestId,
    query,
    refresh,
    selectedNode,
    selectNode,
  };
}
