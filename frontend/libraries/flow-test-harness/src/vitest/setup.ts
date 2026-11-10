// Runs before every flow-test file: browser polyfills jsdom lacks and the
// cleanup that keeps one test's DOM and router calls out of the next.
import { createHash } from 'node:crypto';

import { act, cleanup } from '@testing-library/react';
import { afterAll, afterEach, vi } from 'vitest';

import {
  layoutBoxes,
  layoutRegistry,
  resetLayouts,
  type LayoutWatcher,
} from '../interactions/layout';
import { snapshotPage, takeWrittenSnapshots } from '../interactions/snapshot';
import { resetRouterHarness } from '../router';
import { resetSafeArea } from '../safeArea';
import { installFrameFidelity } from './frames';
import { installWarningGate } from './warningGate';
import { resetStorageHarness } from '../storage';

(
  globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;
installFrameFidelity();

if (!window.matchMedia) {
  window.matchMedia = (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => undefined,
    removeListener: () => undefined,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    dispatchEvent: () => false,
  });
}
if (!(window as any).ResizeObserver) {
  // Recording stub: react-native-web's onLayout relies on ResizeObserver, and
  // the layout driver (interactions/layout.ts) fires the observers it records.
  class ResizeObserverStub implements ResizeObserver {
    private readonly watcher: LayoutWatcher;
    private readonly targets = new Set<Element>();
    constructor(callback: ResizeObserverCallback) {
      this.watcher = { observer: this, callback };
    }
    observe(target: Element): void {
      this.targets.add(target);
      layoutRegistry.watch(target, this.watcher);
    }
    unobserve(target: Element): void {
      this.targets.delete(target);
      layoutRegistry.unwatch(target, this.watcher);
    }
    disconnect(): void {
      for (const target of this.targets)
        layoutRegistry.unwatch(target, this.watcher);
      this.targets.clear();
    }
  }
  (window as unknown as { ResizeObserver: unknown }).ResizeObserver =
    ResizeObserverStub;
}
if (typeof IntersectionObserver === 'undefined') {
  // Tamagui's onLayout polls boxes through IntersectionObserver and switches itself off at import when it is missing,
  // so this must exist before any test file loads Tamagui; it reports the box `session.layout` gave the element.
  class IntersectionObserverStub implements IntersectionObserver {
    readonly root = null;
    readonly rootMargin = '0px';
    readonly scrollMargin = '0px';
    readonly thresholds = [0];
    private readonly targets = new Set<Element>();
    private pending: Element[] = [];
    constructor(private readonly callback: IntersectionObserverCallback) {}
    observe(target: Element): void {
      this.targets.add(target);
      if (this.pending.push(target) > 1) return;
      queueMicrotask(() => {
        const entries = this.takeRecords();
        if (entries.length > 0) this.callback(entries, this);
      });
    }
    unobserve(target: Element): void {
      this.targets.delete(target);
    }
    disconnect(): void {
      this.targets.clear();
      this.pending = [];
    }
    takeRecords(): IntersectionObserverEntry[] {
      const due = this.pending.filter((target) => this.targets.has(target));
      this.pending = [];
      return due.map((target) => {
        const rect = target.getBoundingClientRect();
        const visible =
          rect.right >= 0 &&
          rect.bottom >= 0 &&
          rect.left <= window.innerWidth &&
          rect.top <= window.innerHeight;
        return {
          target,
          boundingClientRect: rect,
          intersectionRect: rect,
          rootBounds: null,
          isIntersecting: visible,
          intersectionRatio: visible ? 1 : 0,
          time: performance.now(),
        };
      });
    }
  }
  (globalThis as { IntersectionObserver: unknown }).IntersectionObserver =
    IntersectionObserverStub;
  (
    window as unknown as { IntersectionObserver: unknown }
  ).IntersectionObserver = IntersectionObserverStub;
}
if (!window.scrollTo) window.scrollTo = () => undefined;
// jsdom has no layout, so element scrolling is a no-op; move the offset and fire `scroll` so scroll-driven controls (wheel pickers, lists) react as in a browser.
function scrollElement(
  this: Element,
  x?: number | ScrollToOptions,
  y?: number
): void {
  const opts = typeof x === 'object' ? x : { left: x, top: y };
  if (opts.left !== undefined) this.scrollLeft = opts.left;
  if (opts.top !== undefined) this.scrollTop = opts.top;
  this.dispatchEvent(new Event('scroll'));
}
Element.prototype.scrollTo = scrollElement;
Element.prototype.scroll = scrollElement;

// Every flow test's final screen is a golden; the hash keeps two long names with one prefix apart.
function finalStep(testName: string): string {
  const hash = createHash('sha1').update(testName).digest('hex').slice(0, 6);
  return `final-${testName.slice(0, 48)}-${hash}`;
}

const QUIET_MS = 300;
const EDITOR_QUIET_MS = 1000;
const SETTLE_CAP_MS = 3000;

// A test may end with renders still queued (CodeMirror highlights in idle time); the golden waits for a quiet DOM.
async function settle(): Promise<void> {
  if (vi.isFakeTimers()) return;
  const frames = Array.from(
    document.querySelectorAll('iframe'),
    (f) => f.contentDocument
  );
  const docs = [document, ...frames.filter((d): d is Document => d !== null)];
  let last = Date.now();
  const observers = docs.map((d) => {
    const o = new MutationObserver(() => {
      last = Date.now();
    });
    o.observe(d, {
      subtree: true,
      childList: true,
      attributes: true,
      characterData: true,
    });
    return o;
  });
  // a language switch loads its parser asynchronously, with no DOM change until the colours arrive
  const quiet = docs.some((d) => d.querySelector('.cm-editor'))
    ? EDITOR_QUIET_MS
    : QUIET_MS;
  const started = Date.now();
  await act(async () => {
    while (Date.now() - last < quiet && Date.now() - started < SETTLE_CAP_MS) {
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
  });
  observers.forEach((o) => o.disconnect());
}

const warnings = installWarningGate();
// registered first so it runs last (hooks unwind as a stack) and also sees what the cleanup below prints
afterEach(() => warnings.check());
afterAll(() => warnings.check());

afterEach(async (ctx) => {
  // every test's last screen is a screenshot; `wf flow run` paints them, and compares them under --goldens
  if (document.body.childElementCount > 0) {
    await settle();
    snapshotPage(finalStep(ctx.task.name), { layouts: layoutBoxes() });
  }
  const snapshots = takeWrittenSnapshots();
  if (snapshots.length > 0) ctx.task.meta.flowSnapshots = snapshots;
  cleanup();
  resetRouterHarness();
  resetStorageHarness();
  resetLayouts();
  resetSafeArea();
});
