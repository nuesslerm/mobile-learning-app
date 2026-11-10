/**
 * Public entry point for the headless `rn-agentation` package. The package no
 * longer ships UI — host apps wrap their tree in {@link AgentationProvider}
 * and read captured state / dispatch taps through {@link useAgentation}.
 *
 * ```tsx
 * <AgentationProvider options={{ endpoint: 'http://localhost:4747' }}>
 *   <App />
 * </AgentationProvider>
 *
 * // Inside <App />:
 * const { annotations, annotateAt, copyToClipboard } = useAgentation();
 * ```
 */

export { AgentationProvider } from './headless/AgentationProvider';
export type { AgentationProviderProps } from './headless/AgentationProvider';

export { useAgentation } from './headless/useAgentation';

export type {
  AgentationHandle,
  AgentationMode,
  AgentationOptions,
} from './headless/types';

// Document node service — runtime element tree querying.
export {
  DocumentNodeService,
  documentNodeToCaptureResult,
} from './document/service/DocumentNodeService';
export { useDocumentNodeService } from './document/hooks/useDocumentNodeService';
export {
  createNativeDocumentTreeDriver,
  createWebDocumentTreeDriver,
} from './document';
export type {
  DocumentNode,
  DocumentNodeQuery,
  DocumentNodeType,
  DocumentTreeDriver,
  DocumentTreeDriverOptions,
  UseDocumentNodeServiceOptions,
  UseDocumentNodeServiceResult,
} from './document';

// Capture + markdown — re-exported so hosts can render their own exports.
export { captureAtPoint } from './capture/capture';
export type { CaptureOptions } from './capture/capture';
export { OVERLAY_DOM_ID } from './capture/capture.web';

export { toMarkdown } from './markdown/toMarkdown';
export { toWireframeMarkdown } from './markdown/toWireframeMarkdown';

export {
  rectToWireframe,
  pointToWireframe,
  ratioToPercent,
  verticalGap,
  DEFAULT_WIREFRAME_WIDTH,
  DEFAULT_WIREFRAME_HEIGHT,
} from './geometry/wireframeGeometry';
export type { Viewport, WireframeRect } from './geometry/wireframeGeometry';

// FAB geometry helpers — useful for host overlays that draw the toggle.
export {
  clamp01,
  fabBounds,
  ratioToPosition,
  positionToRatio,
} from './geometry/fabGeometry';
export type { Insets, FabLayout, FabBounds } from './geometry/fabGeometry';

// API client + session state — exposed so hosts can inspect the persisted state.
export {
  createSession,
  postAnnotation,
} from './api/agentationApi';
export type {
  AgentationHttpConfig,
  AnnotationPayload,
  CreateSessionBody,
  CreateSessionResponse,
} from './api/agentationApi';

export { useAgentationSessionStore } from './state/agentationSessionStore';
export { useFabPositionStore } from './state/fabPositionStore';

// Domain types.
export type {
  AgentationLogger,
  Annotation,
  CaptureResult,
  ParsedComponentName,
  Rect,
  SourceLocation,
  WireframePlacement,
} from './domain/types';
export {
  noopLogger,
  NOISE_COMPONENT_NAMES,
  parseComponentName,
  isUserComponentName,
} from './domain/types';
