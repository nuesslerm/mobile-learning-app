export { createNativeDocumentTreeDriver } from './driver/documentTree.native';
export { createWebDocumentTreeDriver } from './driver/documentTree.web';
export type {
  DocumentNode,
  DocumentNodeQuery,
  DocumentNodeType,
  DocumentTreeDriver,
  DocumentTreeDriverOptions,
  UseDocumentNodeServiceOptions,
} from './domain/types';
export type { UseDocumentNodeServiceResult } from './hooks/useDocumentNodeService';
