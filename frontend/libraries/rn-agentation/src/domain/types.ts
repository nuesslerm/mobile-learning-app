/**
 * Pure domain models for rn-agentation. No React / RN / IO imports here.
 */

/** Source location injected by `@babel/plugin-transform-react-jsx-source` (dev only). */
export interface SourceLocation {
  fileName: string;
  lineNumber: number;
  columnNumber?: number;
}

/** Screen-space bounding box of the tapped element. */
export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** What a platform capture layer resolves from a tap. */
export interface CaptureResult {
  /** Best-guess component / element name (e.g. `PrimaryButton`, `div`). */
  componentName: string;
  /**
   * Nearest-first chain of enclosing user component names (e.g.
   * `["ProblemRow", "ProblemListScreen", "HomeScreen"]`). Greppable by an agent
   * even when an exact `source` isn't available (the web case on React 19).
   */
  ownerChain?: string[];
  /** Exact source file:line, when available (legacy React <19 jsx-source). */
  source?: SourceLocation;
  /**
   * File path (no line) recovered from a React 19 dev display name like
   * `HomeScreen(./(tabs)/index.tsx)`. Coarser than `source` but still points the
   * agent at the right file; present on both web and native in React 19 dev.
   */
  sourceFile?: string;
  /** Bounding box of the element in screen coordinates. */
  rect?: Rect;
  /** A shallow, JSON-safe subset of the element's props, when resolvable. */
  props?: Record<string, unknown>;
}

/**
 * Host/primitive component names that are noise for "which component is this" —
 * skipped when resolving a user-facing component name + owner chain.
 */
export const NOISE_COMPONENT_NAMES = new Set<string>([
  'View',
  'Text',
  'ScrollView',
  'Pressable',
  'TouchableOpacity',
  'TouchableWithoutFeedback',
  'TextInput',
  'Image',
  'RCTView',
  'RCTText',
  'RCTScrollView',
  'VirtualizedList',
  'FlatList',
  'GestureDetector',
  'AnimatedComponent',
  'Animated(View)',
]);

/**
 * A React component display name, split into its clean name and (React 19 dev)
 * the source file embedded in it, e.g. `HomeScreen(./(tabs)/index.tsx)` →
 * `{ name: 'HomeScreen', fileName: '(tabs)/index.tsx' }`.
 */
export interface ParsedComponentName {
  name: string;
  fileName?: string;
}

const DISPLAY_NAME_WITH_FILE = /^(.+?)\((\.{0,2}\/.+)\)$/;

export function parseComponentName(raw: string | undefined): ParsedComponentName {
  if (!raw) return { name: 'unknown' };
  const m = DISPLAY_NAME_WITH_FILE.exec(raw);
  if (m && m[1] && m[2]) {
    return { name: m[1], fileName: m[2].replace(/^\.\//, '') };
  }
  return { name: raw };
}

/** True when a (clean) component name is a real user component worth surfacing. */
export function isUserComponentName(name: string | undefined): name is string {
  if (!name) return false;
  if (name[0] !== name[0]?.toUpperCase()) return false; // lowercase host tag (div, span)
  if (name.startsWith('CssInterop.')) return false;
  if (name.startsWith('Animated.')) return false;
  if (name.startsWith('ForwardRef')) return false;
  if (NOISE_COMPONENT_NAMES.has(name)) return false;
  return true;
}

/** A captured element plus the user's note. */
export interface Annotation extends CaptureResult {
  id: string;
  note: string;
  /** Platform the annotation was captured on. */
  platform: 'web' | 'native';
  /** Annotation kind — regular feedback or wireframe placement. */
  kind?: 'annotate' | 'placement';
  /** Wireframe-only: viewport-relative geometry (0-1 ratios). */
  wireframe?: WireframePlacement;
  /** Whether this annotation is currently selected for editing. */
  selected?: boolean;
  /** Optional document-node context from the runtime tree snapshot. */
  documentNodeId?: string;
  documentNodeProps?: Record<string, unknown>;
  documentNodeSubtree?: unknown;
}


/** Wireframe geometry for a placed component. Ratios are 0-1. */
export interface WireframePlacement {
  componentType: string;
  xRatio: number;
  yRatio: number;
  widthRatio: number;
  heightRatio: number;
  /** Scroll offset at time of placement, so absolute y can be recovered. */
  scrollY?: number;
  /** Optional: the component this placement is associated with. */
  targetComponent?: string;
  /**
   * Per-component editable props (e.g. Button `variant`/`size`/`text`). Set
   * when the wireframe is placed and edited via the host app's annotation
   * dialog. Omitted for legacy annotations that pre-date the editor.
   */
  props?: Record<string, unknown>;
}


/** Minimal logger contract the package depends on (app injects the real one). */
export interface AgentationLogger {
  withMetadata(meta: Record<string, unknown>): { info(event: string): void };
}

/** No-op logger used when the host app does not inject one. */
export const noopLogger: AgentationLogger = {
  withMetadata: () => ({ info: () => {} }),
};
