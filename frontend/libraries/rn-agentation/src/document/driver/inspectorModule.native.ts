// Loaded lazily by documentTree.native.ts. The inspector is a private React Native API; its deep import is what
// triggers React Native's "Deep imports ... are deprecated" warning, so keep it out of the app-start module graph.
const mod = require(
  'react-native/src/private/devsupport/devmenu/elementinspector/getInspectorDataForViewAtPoint',
) as { default: unknown } | unknown;
const rn = require('react-native') as { findNodeHandle?: (ref: unknown) => number | null };

export const inspectorFn = ((typeof mod === 'function' ? mod : (mod as { default?: unknown }).default) ?? null) as
  | ((inspectedView: unknown, x: number, y: number, callback: (viewData: never) => boolean) => void)
  | null;
export const findNodeHandle = rn.findNodeHandle;
