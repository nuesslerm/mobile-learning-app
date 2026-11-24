import { describe, expect, it } from 'vitest';

import { consoleInset, editorZoneMinHeight } from '../layout';

describe('console layout', () => {
  it('Given a root 700 high, When the inset is read, Then it is 490', () => {
    expect(consoleInset(700)).toBe(490);
  });

  it('Given the console open or closed, When the editor zone height is read, Then it is 96 open and 280 closed', () => {
    expect(editorZoneMinHeight(true)).toBe(96);
    expect(editorZoneMinHeight(false)).toBe(280);
  });
});
