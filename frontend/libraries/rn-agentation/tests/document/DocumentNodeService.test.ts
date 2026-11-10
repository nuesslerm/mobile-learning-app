import { describe, expect, it, vi } from 'vitest';
import { DocumentNodeService } from '../../src/document/service/DocumentNodeService';
import type { DocumentNode, DocumentTreeDriver } from '../../src/document/domain/types';

function makeNode(
  id: string,
  name: string,
  bounds: DocumentNode['bounds'],
  children: DocumentNode[] = [],
  props: Record<string, unknown> = {},
): DocumentNode {
  return {
    id,
    type: 'component',
    name,
    bounds,
    props,
    children,
    platform: 'web',
  };
}

function makeDriver(snapshot: DocumentNode | null): DocumentTreeDriver {
  return { takeSnapshot: vi.fn().mockResolvedValue(snapshot) };
}

describe('DocumentNodeService', () => {
  it('takes a snapshot and exposes it', async () => {
    const root = makeNode('root', 'Root', { x: 0, y: 0, width: 100, height: 100 });
    const service = new DocumentNodeService(makeDriver(root));
    await service.refresh();
    expect(service.currentSnapshot?.id).toBe('root');
  });

  it('queries by point', async () => {
    const child = makeNode('child', 'Child', { x: 10, y: 10, width: 20, height: 20 });
    const root = makeNode('root', 'Root', { x: 0, y: 0, width: 100, height: 100 }, [child]);
    const service = new DocumentNodeService(makeDriver(root));
    await service.refresh();
    const found = await service.queryAtPoint(15, 15);
    expect(found?.id).toBe('child');
  });

  it('queries by name', async () => {
    const a = makeNode('a', 'A', { x: 0, y: 0, width: 1, height: 1 });
    const b = makeNode('b', 'B', { x: 0, y: 0, width: 1, height: 1 });
    const root = makeNode('root', 'Root', { x: 0, y: 0, width: 100, height: 100 }, [a, b]);
    const service = new DocumentNodeService(makeDriver(root));
    await service.refresh();
    expect(service.queryByName('A').length).toBe(1);
    expect(service.queryByName('B')[0]!.id).toBe('b');
  });

  it('queries by test id', async () => {
    const a = makeNode('a', 'A', { x: 0, y: 0, width: 1, height: 1 }, [], { testID: 'foo' });
    const root = makeNode('root', 'Root', { x: 0, y: 0, width: 100, height: 100 }, [a]);
    const service = new DocumentNodeService(makeDriver(root));
    await service.refresh();
    const found = service.queryByTestId('foo');
    expect(found.length).toBe(1);
    expect(found[0]!.id).toBe('a');
  });

  it('selects a node', async () => {
    const child = makeNode('child', 'Child', { x: 0, y: 0, width: 1, height: 1 });
    const root = makeNode('root', 'Root', { x: 0, y: 0, width: 100, height: 100 }, [child]);
    const service = new DocumentNodeService(makeDriver(root));
    await service.refresh();
    service.selectNode('child');
    expect(service.selectedNode?.id).toBe('child');
  });

  it('notifies subscribers on selection', async () => {
    const root = makeNode('root', 'Root', { x: 0, y: 0, width: 100, height: 100 });
    const service = new DocumentNodeService(makeDriver(root));
    await service.refresh();
    let observedSelectedId: string | undefined;
    const fn = vi.fn(() => {
      observedSelectedId = service.selectedNode?.id;
    });
    service.subscribe(fn);
    service.selectNode('root');
    expect(fn).toHaveBeenCalled();
    // Assert on the real state observed inside the callback -- proves the
    // notification fires AFTER the selection is applied, not just that a
    // notification happened.
    expect(observedSelectedId).toBe('root');
  });

  it('reflects the latest snapshot when queryAtPoint runs after refresh()', async () => {
    // Covers the `annotateAt` pattern: refresh(), then queryAtPoint. The
    // driver returns a different tree on each call (e.g. a debug sheet that
    // appeared between taps); the second `queryAtPoint` must see the new
    // tree, not the stale cached snapshot from the first refresh.
    const firstChild = makeNode('a', 'A', { x: 10, y: 10, width: 1, height: 1 });
    const firstRoot = makeNode('first', 'First', { x: 0, y: 0, width: 100, height: 100 }, [firstChild]);
    const secondChild = makeNode('b', 'B', { x: 10, y: 10, width: 1, height: 1 });
    const secondRoot = makeNode('second', 'Second', { x: 0, y: 0, width: 100, height: 100 }, [secondChild]);

    const driver: DocumentTreeDriver = {
      takeSnapshot: vi
        .fn()
        .mockResolvedValueOnce(firstRoot)
        .mockResolvedValueOnce(secondRoot),
    };

    const service = new DocumentNodeService(driver);
    await service.refresh();
    const cached = await service.queryAtPoint(10, 10);
    expect(cached?.id).toBe('a');

    // Refresh (the line `annotateAt` runs before every queryAtPoint). The
    // driver now returns the new tree; the next query must reflect it.
    await service.refresh();
    const fresh = await service.queryAtPoint(10, 10);
    expect(fresh?.id).toBe('b');
  });
});
