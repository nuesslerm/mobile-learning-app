import type { AgentationLogger, Rect, SourceLocation } from '../../domain/types';

export type DocumentNodeType = 'component' | 'element';

export interface DocumentNode {
  id: string;
  type: DocumentNodeType;
  name: string;
  displayName?: string;
  source?: SourceLocation;
  sourceFile?: string;
  bounds: Rect;
  props: Record<string, unknown>;
  children: DocumentNode[];
  parentId?: string;
  ownerChain?: string[];
  platform: 'web' | 'native';
}

export interface DocumentTreeDriverOptions {
  inspectedView?: unknown;
  logger?: AgentationLogger;
  /**
   * CSS selectors whose matched subtrees are excluded from the snapshot.
   * Useful for skipping dev-only UI (debug sheets, in-app overlays) that
   * would otherwise sit on top of app content and be hit-tested instead of
   * the underlying element. The hard-coded overlay selector is always
   * applied in addition to anything provided here.
   */
  ignoredSelectors?: string[];
}

export interface DocumentTreeDriver {
  takeSnapshot(): Promise<DocumentNode | null>;
  /**
   * Optional native hit-test: resolve the document node at the given viewport
   * coordinates. When supported, the driver walks the live scene using its
   * platform's own hit-test (e.g. `document.elementsFromPoint` on web) instead
   * of bounding-box DFS over the snapshot tree, which would otherwise pick the
   * topmost node in DOM order — the bug that lets a debug sheet or any
   * overlay swallow taps meant for content underneath.
   *
   * Callers (`DocumentNodeService`) fall back to `findDeepestAtPoint` over the
   * snapshot tree when this method is absent (e.g. native drivers without a
   * point-query primitive).
   */
  queryAtPoint?(x: number, y: number): Promise<DocumentNode | undefined>;
}

export type DocumentNodeQuery =
  | { kind: 'atPoint'; x: number; y: number }
  | { kind: 'byName'; name: string }
  | { kind: 'bySource'; file: string }
  | { kind: 'byTestId'; testId: string }
  | { kind: 'predicate'; predicate: (node: DocumentNode) => boolean };

export interface UseDocumentNodeServiceOptions {
  driver?: DocumentTreeDriver;
  logger?: AgentationLogger;
}
