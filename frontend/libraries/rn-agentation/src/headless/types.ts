/**
 * Public types for the headless `rn-agentation` API.
 *
 * The package no longer renders any UI itself. Host apps wire up their own FAB
 * and overlay (see `examples/`); they read captured state and dispatch taps
 * through `useAgentation`.
 */
import type * as React from 'react';
import type { View } from 'react-native';

import type {
  AgentationLogger,
  Annotation,
} from '../domain/types';
import type { DocumentTreeDriverOptions } from '../document/domain/types';

export type AgentationMode = 'annotate' | 'placement';

/** Options accepted by {@link AgentationProvider}. */
export interface AgentationOptions {
  /** Master switch — when false, capture / place / wireframe calls become no-ops. Default true. */
  enabled?: boolean;
  /** Host app logger (structured wide events). Defaults to a no-op. */
  logger?: AgentationLogger;
  /**
   * Native only: a ref to the app-content root view. The capture hit-test is
   * scoped to this subtree so the host overlay is never resolved. On web this
   * is ignored (DOM hit-testing skips the overlay by id).
   */
  inspectedRef?: React.RefObject<View | null>;
  /**
   * Optional agentation-mcp HTTP endpoint. When set, each annotation is posted
   * to the server so the host agent can read it back via MCP tools.
   */
  endpoint?: string;
  /**
   * Forwarded to the document tree driver. On web, `ignoredSelectors` lets the
   * host mark dev-only UI (debug sheets, FABs) so the snapshot skips those
   * subtrees and the hit-test resolves to the underlying app element.
   */
  documentTreeDriverOptions?: DocumentTreeDriverOptions;
}

/** Public interface returned by {@link useAgentation}. */
export interface AgentationHandle {
  active: boolean;
  annotations: Annotation[];
  selectedId: string | null;
  mode: AgentationMode;
  selectedType: string;
  panelOpen: boolean;
  placementPending: boolean;

  toggleActive: () => void;
  setMode: (mode: AgentationMode) => void;
  setSelectedType: (type: string) => void;
  setPanelOpen: (open: boolean) => void;
  startPlacement: () => void;
  cancelPlacement: () => void;

  /** Capture the element under (x, y) and append an annotation. */
  annotateAt: (x: number, y: number) => Promise<void>;
  /** Place a default-sized wireframe of the currently selected type at (x, y). */
  placeWireframeAt: (x: number, y: number) => void;
  /** Nudge a wireframe annotation by pixel deltas (converted via current viewport). */
  moveWireframe: (id: string, dx: number, dy: number) => void;
  /** Resize a wireframe annotation by pixel deltas (converted via current viewport). */
  resizeWireframe: (id: string, dw: number, dh: number) => void;
  /** Mark an annotation as selected (highlight + edit). */
  selectAnnotation: (id: string) => void;
  /** Update the free-text note attached to an annotation. */
  updateNote: (id: string, note: string) => void;
  /**
   * Merge the given props into a wireframe annotation's existing props.
   * No-op if the annotation isn't a wireframe (no `props` field to merge into).
   */
  updateWireframeProps: (id: string, props: Record<string, unknown>) => void;
  /** Remove an annotation from the session by id. Clears the selection if it matched. */
  deleteAnnotation: (id: string) => void;
  /** Render the current annotations to markdown and copy to clipboard. */
  copyToClipboard: () => Promise<void>;
  /** Clear all annotations in the current session. */
  clear: () => void;
}
