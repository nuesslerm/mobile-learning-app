// Harness layout driver: give an element a box and poke every ResizeObserver
// watching it, so react-native-web's own onLayout path measures and fires.
// The vitest ResizeObserver stub (vitest/setup.ts) feeds this registry.
export type LayoutBox = {
  x?: number;
  y?: number;
  width: number;
  height: number;
};

export type LayoutWatcher = {
  observer: ResizeObserver;
  callback: ResizeObserverCallback;
};

const boxes = new WeakMap<Element, LayoutBox>();
const observed = new WeakMap<Element, Set<LayoutWatcher>>();
const originals = new WeakMap<
  Element,
  Map<string, PropertyDescriptor | undefined>
>();
const overriddenElements = new Set<Element>();
const observedElements = new Set<Element>();

const BOX_GETTERS = [
  'offsetWidth',
  'offsetHeight',
  'clientWidth',
  'clientHeight',
] as const;
const OVERRIDDEN_PROPS = ['getBoundingClientRect', ...BOX_GETTERS] as const;

function rectFromBox(box: LayoutBox): DOMRect {
  const x = box.x ?? 0;
  const y = box.y ?? 0;
  const width = box.width;
  const height = box.height;
  return {
    x,
    y,
    width,
    height,
    top: y,
    left: x,
    bottom: y + height,
    right: x + width,
    toJSON: () => ({}),
  };
}

function entryFor(el: Element): ResizeObserverEntry {
  const box = boxes.get(el);
  return {
    target: el,
    contentRect: box ? rectFromBox(box) : rectFromBox({ width: 0, height: 0 }),
  } as ResizeObserverEntry;
}

function overrideElement(el: Element): void {
  if (originals.has(el)) return;
  const saved = new Map<string, PropertyDescriptor | undefined>();
  for (const prop of OVERRIDDEN_PROPS)
    saved.set(prop, Object.getOwnPropertyDescriptor(el, prop));
  originals.set(el, saved);
  overriddenElements.add(el);
  Object.defineProperty(el, 'getBoundingClientRect', {
    configurable: true,
    value: () => rectFromBox(boxes.get(el) ?? { width: 0, height: 0 }),
  });
  for (const prop of BOX_GETTERS) {
    Object.defineProperty(el, prop, {
      configurable: true,
      get: () => {
        const box = boxes.get(el);
        if (!box) return 0;
        return prop === 'offsetWidth' || prop === 'clientWidth'
          ? box.width
          : box.height;
      },
    });
  }
}

function restoreElement(el: Element): void {
  const saved = originals.get(el);
  if (saved) {
    for (const [prop, descriptor] of saved) {
      if (descriptor) Object.defineProperty(el, prop, descriptor);
      else Reflect.deleteProperty(el, prop);
    }
  }
  originals.delete(el);
  boxes.delete(el);
  overriddenElements.delete(el);
}

function fire(el: Element): void {
  const watchers = observed.get(el);
  if (!watchers || watchers.size === 0) return;
  const entry = entryFor(el);
  for (const { observer, callback } of watchers) callback([entry], observer);
}

export function setLayout(el: Element, box: LayoutBox): void {
  overrideElement(el);
  boxes.set(el, box);
  fire(el);
}

export function layoutBoxes(): ReadonlyMap<Element, LayoutBox> {
  const entries = new Map<Element, LayoutBox>();
  for (const el of overriddenElements) {
    const box = boxes.get(el);
    if (box) entries.set(el, box);
  }
  return entries;
}

export function resetLayouts(): void {
  for (const el of [...overriddenElements]) restoreElement(el);
  for (const el of [...observedElements]) fire(el);
}

export const layoutRegistry = {
  /** Record an observer as watching `el`; report the stored box at once if one exists. */
  watch(el: Element, watcher: LayoutWatcher): void {
    let watchers = observed.get(el);
    if (!watchers) {
      watchers = new Set();
      observed.set(el, watchers);
    }
    watchers.add(watcher);
    observedElements.add(el);
    if (boxes.has(el)) watcher.callback([entryFor(el)], watcher.observer);
  },
  unwatch(el: Element, watcher: LayoutWatcher): void {
    const watchers = observed.get(el);
    if (!watchers) return;
    watchers.delete(watcher);
    if (watchers.size === 0) observedElements.delete(el);
  },
};
