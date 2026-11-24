import { describe, expect, it } from 'vitest';
import { createNativeDocumentTreeDriver } from '../../src/document/driver/documentTree.native';

describe('createNativeDocumentTreeDriver', () => {
  it('Given an inspector that cannot load, When a snapshot is taken, Then it is null and the reason is logged', async () => {
    const events: string[] = [];
    const logger = { withMetadata: () => ({ info: (event: string) => events.push(event) }) };
    const driver = createNativeDocumentTreeDriver({
      logger,
      loadInspector: () => Promise.reject(new Error('not found')),
    });

    const snapshot = await driver.takeSnapshot();

    expect(snapshot).toBeNull();
    expect(events).toEqual(['agentation.native_driver.unavailable']);
  });
});
