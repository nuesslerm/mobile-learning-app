import { describe, expect, it, vi } from 'vitest';
import { createNativeDocumentTreeDriver } from '../../src/document/driver/documentTree.native';

describe('createNativeDocumentTreeDriver', () => {
  it('returns null when inspector module is unavailable', async () => {
    vi.doMock('react-native', () => ({}));
    vi.doMock(
      'react-native/src/private/devsupport/devmenu/elementinspector/getInspectorDataForViewAtPoint',
      () => {
        throw new Error('not found');
      },
    );
    const driver = createNativeDocumentTreeDriver();
    const snapshot = await driver.takeSnapshot();
    expect(snapshot).toBeNull();
  });
});
