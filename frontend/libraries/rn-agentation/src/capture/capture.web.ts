import { createWebDocumentTreeDriver, OVERLAY_DOM_ID } from '../document/driver/documentTree.web';
import { documentNodeToCaptureResult } from '../document/service/DocumentNodeService';
import { noopLogger } from '../domain/types';
import type { CaptureResult } from '../domain/types';
import type { CaptureOptions } from './capture';

export { OVERLAY_DOM_ID };

export async function captureAtPoint(
  locationX: number,
  locationY: number,
  options?: CaptureOptions,
): Promise<CaptureResult> {
  const logger = options?.logger ?? noopLogger;
  const driver = createWebDocumentTreeDriver({ logger });
  const snapshot = await driver.takeSnapshot();
  if (!snapshot) {
    return { componentName: 'unknown' };
  }
  const node = findNodeAtPoint(snapshot, locationX, locationY);
  return node ? documentNodeToCaptureResult(node) : { componentName: 'unknown' };
}

function findNodeAtPoint(
  node: import('../document/domain/types').DocumentNode,
  x: number,
  y: number,
): import('../document/domain/types').DocumentNode | undefined {
  const contains =
    x >= node.bounds.x &&
    x <= node.bounds.x + node.bounds.width &&
    y >= node.bounds.y &&
    y <= node.bounds.y + node.bounds.height;
  if (!contains) return undefined;
  for (const child of node.children) {
    const found = findNodeAtPoint(child, x, y);
    if (found) return found;
  }
  return node;
}
