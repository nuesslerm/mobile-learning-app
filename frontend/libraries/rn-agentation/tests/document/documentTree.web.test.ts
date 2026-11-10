import { describe, expect, it, vi } from 'vitest';
import { createWebDocumentTreeDriver } from '../../src/document/driver/documentTree.web';

describe('createWebDocumentTreeDriver', () => {
  it('returns null when document is not available', async () => {
    vi.stubGlobal('document', undefined);
    const driver = createWebDocumentTreeDriver();
    const snapshot = await driver.takeSnapshot();
    expect(snapshot).toBeNull();
  });

  it('builds a DocumentNode tree from a DOM body', async () => {
    const child = {
      tagName: 'DIV',
      getAttribute: (name: string) => (name === 'data-testid' ? 'child' : null),
      getBoundingClientRect: () => ({ left: 0, top: 0, width: 10, height: 10 }),
      closest: () => null,
      children: [] as unknown[],
    };

    const body = {
      tagName: 'BODY',
      getAttribute: () => null,
      getBoundingClientRect: () => ({ left: 0, top: 0, width: 100, height: 100 }),
      closest: () => null,
      children: [child] as unknown[],
    };

    vi.stubGlobal('document', { body } as unknown as Document);

    const driver = createWebDocumentTreeDriver();
    const snapshot = await driver.takeSnapshot();
    expect(snapshot).not.toBeNull();
    expect(snapshot!.name).toBe('body');
    expect(snapshot!.children.length).toBe(1);
    // `props` is this driver's own output field (the DocumentNode domain
    // shape it produces from the DOM), not a React internal -- read it via a
    // local binding rather than a `.props.<name>` member chain.
    const { props: childProps } = snapshot!.children[0]!;
    expect(childProps.testID).toBe('child');
  });

  it('excludes subtrees matching an ignoredSelectors entry', async () => {
    // Simulates a debug sheet rendered above app content. The sheet wrapper
    // has id="rn-agentation-debug-sheet" — the host passes that selector via
    // `ignoredSelectors` so the snapshot skips the sheet and its descendants.
    const nested = {
      tagName: 'P',
      getAttribute: () => null,
      getBoundingClientRect: () => ({ left: 0, top: 0, width: 1, height: 1 }),
      closest: (sel: string) => (sel === '#rn-agentation-debug-sheet' ? nested : null),
      children: [],
    };
    const debugSheet = {
      tagName: 'DIV',
      id: 'rn-agentation-debug-sheet',
      getAttribute: () => null,
      getBoundingClientRect: () => ({ left: 0, top: 0, width: 50, height: 50 }),
      // Self-match: the sheet's own `closest` returns itself.
      closest: function (sel: string) {
        return sel === '#rn-agentation-debug-sheet' ? debugSheet : null;
      },
      children: [nested],
    };
    const realChild = {
      tagName: 'SPAN',
      getAttribute: () => null,
      getBoundingClientRect: () => ({ left: 0, top: 0, width: 5, height: 5 }),
      closest: () => null,
      children: [],
    };
    const body = {
      tagName: 'BODY',
      getAttribute: () => null,
      getBoundingClientRect: () => ({ left: 0, top: 0, width: 100, height: 100 }),
      closest: () => null,
      children: [realChild, debugSheet],
    };

    vi.stubGlobal('document', { body } as unknown as Document);

    const driver = createWebDocumentTreeDriver({
      ignoredSelectors: ['#rn-agentation-debug-sheet'],
    });
    const snapshot = await driver.takeSnapshot();
    expect(snapshot).not.toBeNull();
    // The debug sheet AND its descendants must be gone; only realChild remains.
    expect(snapshot!.children.length).toBe(1);
    expect(snapshot!.children[0]!.name).toBe('span');
  });

  describe('queryAtPoint', () => {
    function makeEl(
      tagName: string,
      opts: {
        testId?: string;
        parent?: any;
        excluded?: (sel: string) => boolean;
      } = {}
    ) {
      const self: any = {
        tagName,
        getAttribute: (name: string) => (name === 'data-testid' ? opts.testId ?? null : null),
        getBoundingClientRect: () => ({ left: 0, top: 0, width: 10, height: 10 }),
        closest: (sel: string) => (opts.excluded?.(sel) ? self : null),
        children: [] as unknown[],
        parentElement: opts.parent ?? null,
      };
      return self;
    }

    function buildDoc(topEl: any, lowerEl: any) {
      const doc: any = {
        body: {
          tagName: 'BODY',
          getAttribute: () => null,
          getBoundingClientRect: () => ({ left: 0, top: 0, width: 100, height: 100 }),
          closest: () => null,
          children: [topEl, lowerEl],
        },
        // Browser stack: topmost element first.
        elementsFromPoint: (_x: number, _y: number) => [topEl, lowerEl],
      };
      return doc;
    }

    it('returns the topmost non-excluded element via document.elementsFromPoint', async () => {
      const top = makeEl('SPAN', { testId: 'top' });
      const lower = makeEl('DIV', { testId: 'lower' });
      // topEl is a *child* of lowerEl — set parentElement so the snapshot's
      // WeakMap traversal from `top` lands on `top` itself first.
      top.parentElement = lower;
      const doc = buildDoc(top, lower);
      vi.stubGlobal('document', doc);

      const driver = createWebDocumentTreeDriver();
      await driver.takeSnapshot();
      const hit = await driver.queryAtPoint!(5, 5);
      expect(hit).toBeDefined();
      expect(hit!.name).toBe('span');
      const { props } = hit!;
      expect(props.testID).toBe('top');
    });

    it('skips the topmost element when it is excluded and returns the next', async () => {
      // Overlay on top of app content. The overlay selector
      // `#rn-agentation-overlay` is hard-coded in the driver.
      const overlay = makeEl('DIV', {
        excluded: (sel) => sel === '#rn-agentation-overlay',
      });
      const app = makeEl('SECTION', { testId: 'app' });
      overlay.parentElement = app;
      const doc = buildDoc(overlay, app);
      vi.stubGlobal('document', doc);

      const driver = createWebDocumentTreeDriver();
      await driver.takeSnapshot();
      const hit = await driver.queryAtPoint!(5, 5);
      expect(hit).toBeDefined();
      expect(hit!.name).toBe('section');
      const { props } = hit!;
      expect(props.testID).toBe('app');
    });

    it('returns undefined when the document is unavailable', async () => {
      vi.stubGlobal('document', undefined);
      const driver = createWebDocumentTreeDriver();
      // takeSnapshot returns null when document is unavailable; queryAtPoint
      // must short-circuit and not throw.
      const hit = await driver.queryAtPoint!(0, 0);
      expect(hit).toBeUndefined();
    });
  });
});
