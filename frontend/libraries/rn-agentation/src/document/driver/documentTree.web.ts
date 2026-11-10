import type { AgentationLogger, SourceLocation } from '../../domain/types';
import { noopLogger } from '../../domain/types';
import type { DocumentNode, DocumentTreeDriver, DocumentTreeDriverOptions } from '../domain/types';

export const OVERLAY_DOM_ID = 'rn-agentation-overlay';

interface FiberSource {
  fileName?: string;
  lineNumber?: number;
  columnNumber?: number;
}

interface Fiber {
  return?: Fiber | null;
  _debugOwner?: Fiber | null;
  _debugSource?: FiberSource | null;
  memoizedProps?: Record<string, unknown> | null;
  type?: unknown;
  elementType?: unknown;
}

function getFiber(node: Element): Fiber | undefined {
  const key = Object.keys(node).find(
    (k) => k.startsWith('__reactFiber$') || k.startsWith('__reactInternalInstance$'),
  );
  return key ? ((node as unknown as Record<string, Fiber>)[key]) : undefined;
}

function sourceFromFiber(fiber: Fiber): FiberSource | undefined {
  const fromProps = fiber.memoizedProps?.__source as FiberSource | undefined;
  if (fromProps?.fileName && fromProps.lineNumber != null) return fromProps;
  if (fiber._debugSource?.fileName && fiber._debugSource.lineNumber != null) return fiber._debugSource;
  return undefined;
}

function componentNameFromFiber(fiber: Fiber): string | undefined {
  const t = (fiber.elementType ?? fiber.type) as { displayName?: string; name?: string } | string | undefined;
  if (typeof t === 'string') return t;
  if (t && (t.displayName || t.name)) return t.displayName ?? t.name;
  return undefined;
}

function normalizeFileName(fileName: string): string {
  const marker = '/frontend/';
  const idx = fileName.lastIndexOf(marker);
  return idx >= 0 ? fileName.slice(idx + 1) : fileName;
}

function toSource(s: FiberSource | undefined): SourceLocation | undefined {
  if (!s?.fileName || s.lineNumber == null) return undefined;
  return {
    fileName: normalizeFileName(s.fileName),
    lineNumber: s.lineNumber,
    columnNumber: s.columnNumber,
  };
}

function isUserComponentName(name: string): boolean {
  if (!name) return false;
  const first = name[0];
  if (!first) return false;
  if (first !== first.toUpperCase()) return false;
  if (name.startsWith('CssInterop.')) return false;
  if (name.startsWith('Animated.')) return false;
  if (name.startsWith('ForwardRef')) return false;
  const noise = new Set([
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
  return !noise.has(name);
}

function parseDisplayName(raw: string): { name: string; fileName?: string } {
  const m = /^(.+?)\((\.{0,2}\/.+)\)$/.exec(raw);
  if (m && m[1] && m[2]) {
    return { name: m[1], fileName: m[2].replace(/^\.\//, '') };
  }
  return { name: raw };
}

function nodeFromElement(el: Element, platform: 'web', logger: AgentationLogger): DocumentNode | null {
  try {
    const rectDom = el.getBoundingClientRect();
    const bounds = {
      x: rectDom.left,
      y: rectDom.top,
      width: rectDom.width,
      height: rectDom.height,
    };
    const testId = el.getAttribute('data-testid') ?? undefined;
    const props: Record<string, unknown> = {};
    if (testId) props.testID = testId;

    let source: SourceLocation | undefined;
    let sourceFile: string | undefined;
    let name = el.tagName.toLowerCase();
    let displayName: string | undefined;
    let type: 'component' | 'element' = 'element';
    const ownerChain: string[] = [];
    const fiber = getFiber(el);
    if (fiber) {
      let f: Fiber | undefined = fiber;
      let guard = 0;
      while (f && guard < 200) {
        guard++;
        if (!source) source = toSource(sourceFromFiber(f));
        const rawName = componentNameFromFiber(f);
        if (rawName) {
          const parsed = parseDisplayName(rawName);
          if (isUserComponentName(parsed.name)) {
            if (name === el.tagName.toLowerCase()) {
              name = parsed.name;
              type = 'component';
              displayName = rawName;
            }
            if (ownerChain[ownerChain.length - 1] !== parsed.name) {
              ownerChain.push(parsed.name);
            }
            if (!sourceFile && parsed.fileName) sourceFile = parsed.fileName;
          }
        }
        f = f._debugOwner ?? f.return ?? undefined;
      }
    }

    return {
      id: `${platform}-${el.tagName}-${Math.random().toString(36).slice(2, 9)}`,
      type,
      name,
      displayName,
      source,
      sourceFile,
      bounds,
      props,
      children: [],
      ownerChain: ownerChain.length > 0 ? ownerChain.slice(0, 8) : undefined,
      platform,
    };
  } catch (err) {
    logger.withMetadata({ err: String(err) }).info('agentation.web_driver.node_failed');
    return null;
  }
}

export function createWebDocumentTreeDriver(options?: DocumentTreeDriverOptions): DocumentTreeDriver {
  const logger = options?.logger ?? noopLogger;
  // Hard-coded overlay + caller-supplied selectors. Invalid selectors throw
  // from `Element.closest`, so we filter them defensively before each call.
  const ignoredSelectors: string[] = [`#${OVERLAY_DOM_ID}`, ...(options?.ignoredSelectors ?? [])];

  const isExcluded = (el: Element): boolean => {
    for (const selector of ignoredSelectors) {
      try {
        if (el.closest(selector)) return true;
      } catch {
        // Skip invalid selectors silently — one bad caller value shouldn't
        // break every snapshot.
      }
    }
    return false;
  };

  // Each non-excluded element we materialise during a snapshot is mapped to
  // its generated `DocumentNode` so `queryAtPoint` can resolve the browser's
  // native hit-test (which yields raw Elements) back to the snapshot's node
  // identity. Populated only during `takeSnapshot`; `DocumentNodeService`
  // refreshes before querying, so this map is always current at the time of
  // the lookup. Cleared on every snapshot to drop stale DOM references.
  let nodeByElement: WeakMap<Element, DocumentNode> | null = null;

  return {
    async takeSnapshot() {
      if (typeof document === 'undefined') return null;
      const root = document.body;
      if (!root) return null;

      const map: WeakMap<Element, DocumentNode> = new WeakMap();

      const walk = (el: Element): DocumentNode | null => {
        if (isExcluded(el)) return null;
        const node = nodeFromElement(el, 'web', logger);
        if (!node) return null;
        map.set(el, node);
        for (const child of Array.from(el.children)) {
          const childNode = walk(child);
          if (childNode) {
            childNode.parentId = node.id;
            node.children.push(childNode);
          }
        }
        return node;
      };

      nodeByElement = map;
      return walk(root);
    },

    async queryAtPoint(x, y) {
      if (typeof document === 'undefined') return undefined;
      const map = nodeByElement;
      if (!map) return undefined;

      // Prefer `elementsFromPoint` (top-to-bottom stack) — it reflects z-index,
      // opacity, and pointer-events the way the user actually perceives them.
      // Fall back to `elementFromPoint` only on environments that lack the
      // stack variant (very old browsers / non-DOM test envs).
      const stack: Element[] =
        typeof document.elementsFromPoint === 'function'
          ? document.elementsFromPoint(x, y)
          : document.elementFromPoint(x, y)
            ? [document.elementFromPoint(x, y) as Element]
            : [];

      for (const el of stack) {
        if (!el) continue;
        // Excluded ancestors (overlay / debug sheet) are removed from the
        // snapshot, so walking back up `parentElement` finds the first ancestor
        // that *is* in the map — that is the topmost non-excluded node the user
        // can actually see / tap.
        let cursor: Element | null = el;
        while (cursor) {
          if (isExcluded(cursor)) break;
          const mapped = map.get(cursor);
          if (mapped) return mapped;
          cursor = cursor.parentElement;
        }
      }
      return undefined;
    },
  };
}
