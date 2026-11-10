import type { AgentationLogger, SourceLocation } from '../../domain/types';
import { noopLogger } from '../../domain/types';
import type { DocumentNode, DocumentTreeDriver, DocumentTreeDriverOptions } from '../domain/types';

declare const require: (id: string) => unknown;

interface InspectorSource {
  fileName?: string;
  lineNumber?: number;
  columnNumber?: number;
}

interface InspectorData {
  source?: InspectorSource | null;
  props?: Record<string, unknown> | null;
  measure?: unknown;
}

interface HierarchyItem {
  name?: string;
  getInspectorData: (findNodeHandle: unknown) => InspectorData;
}

interface Frame {
  left: number;
  top: number;
  width: number;
  height: number;
}

interface TouchedViewData {
  frame?: Frame;
  hierarchy: HierarchyItem[];
}

type GetInspectorDataForViewAtPoint = (
  inspectedView: unknown,
  locationX: number,
  locationY: number,
  callback: (viewData: TouchedViewData) => boolean,
) => void;

let cachedFn: GetInspectorDataForViewAtPoint | null | undefined;
let cachedFindNodeHandle: ((ref: unknown) => number | null) | undefined;

async function loadInspector(logger: AgentationLogger): Promise<GetInspectorDataForViewAtPoint | null> {
  if (cachedFn !== undefined) return cachedFn;
  try {
    // A separate module on purpose: React Native warns about its private deep
    // import when the module holding the require() first runs, so only a real
    // inspection pays for it instead of every app start.
    const loaded = await import('./inspectorModule.native');
    cachedFn = loaded.inspectorFn;
    cachedFindNodeHandle = loaded.findNodeHandle;
  } catch (err) {
    logger.withMetadata({ err: String(err) }).info('agentation.native_driver.unavailable');
    cachedFn = null;
  }
  return cachedFn;
}

function normalizeFileName(fileName: string): string {
  const marker = '/frontend/';
  const idx = fileName.lastIndexOf(marker);
  return idx >= 0 ? fileName.slice(idx + 1) : fileName;
}

function toSource(s: InspectorSource | null | undefined): SourceLocation | undefined {
  if (!s || !s.fileName || s.lineNumber == null) return undefined;
  return {
    fileName: normalizeFileName(s.fileName),
    lineNumber: s.lineNumber,
    columnNumber: s.columnNumber,
  };
}

function parseName(raw?: string): { name: string; fileName?: string } {
  if (!raw) return { name: 'unknown' };
  const m = /^(.+?)\((\.{0,2}\/.+)\)$/.exec(raw);
  if (m && m[1] && m[2]) return { name: m[1], fileName: m[2].replace(/^\.\//, '') };
  return { name: raw };
}

function isUserComponentName(name: string): boolean {
  if (!name) return false;
  const first = name[0];
  if (!first || first !== first.toUpperCase()) return false;
  if (name.startsWith('CssInterop.') || name.startsWith('Animated.') || name.startsWith('ForwardRef')) return false;
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

function pickProps(props: Record<string, unknown> | null | undefined): Record<string, unknown> | undefined {
  if (!props) return undefined;
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(props)) {
    if (k === 'children' || k === '__source' || k === '__self') continue;
    const t = typeof v;
    if (t === 'string' || t === 'number' || t === 'boolean') out[k] = v;
  }
  return Object.keys(out).length > 0 ? out : undefined;
}

export function createNativeDocumentTreeDriver(options?: DocumentTreeDriverOptions): DocumentTreeDriver {
  const logger = options?.logger ?? noopLogger;
  const inspectedView = options?.inspectedView ?? null;
  return {
    async takeSnapshot() {
      const fn = await loadInspector(logger);
      if (!fn) return null;

      return new Promise<DocumentNode | null>((resolve) => {
        let settled = false;
        const settle = (r: DocumentNode | null): boolean => {
          if (!settled) {
            settled = true;
            resolve(r);
          }
          return true;
        };

        try {
          // The inspector API is point-based; to build a full-tree snapshot we
          // seed it with a hit at (0,0) to recover the root frame + hierarchy.
          fn(inspectedView, 0, 0, (viewData) => {
            const hierarchy = viewData?.hierarchy ?? [];
            const frame = viewData?.frame;
            const bounds = frame
              ? { x: frame.left, y: frame.top, width: frame.width, height: frame.height }
              : { x: 0, y: 0, width: 0, height: 0 };

            const rootId = `native-root-${Math.random().toString(36).slice(2, 9)}`;
            const root: DocumentNode = {
              id: rootId,
              type: 'element',
              name: 'root',
              bounds,
              props: {},
              children: [],
              platform: 'native',
            };

            let currentParent = root;
            for (let i = 0; i < hierarchy.length; i++) {
              const item = hierarchy[i];
              if (!item) continue;
              const parsed = parseName(item.name);
              if (!isUserComponentName(parsed.name)) continue;
              let data: InspectorData | undefined;
              try {
                data = item.getInspectorData(cachedFindNodeHandle);
              } catch {
                data = undefined;
              }
              const source = toSource(data?.source);
              const props = pickProps(data?.props) ?? {};
              const node: DocumentNode = {
                id: `native-${parsed.name}-${i}`,
                type: 'component',
                name: parsed.name,
                displayName: item.name,
                source,
                sourceFile: parsed.fileName,
                bounds: { x: 0, y: 0, width: 0, height: 0 },
                props,
                children: [],
                parentId: currentParent.id,
                platform: 'native',
              };
              currentParent.children.push(node);
              currentParent = node;
            }
            return settle(root);
          });
        } catch (err) {
          logger.withMetadata({ err: String(err) }).info('agentation.native_driver.snapshot_failed');
          settle(null);
        }
        if (!settled) settle(null);
      });
    },
  };
}
