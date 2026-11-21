import { describe, expect, it } from 'vitest';

import { createFakeEditor } from '../../src/testing';

describe('the fake editor host', () => {
  it('given a fake built with tree, when treeOf is asked for another text, then the snapshot covers that text and the document, the revision and the undo stack are unchanged', () => {
    const fake = createFakeEditor({ text: 'a = 1\n', tree: true });
    const other = 'def f(x):\n    return x\n';

    const snapshot = fake.host.treeOf?.(other);

    expect(snapshot?.length).toBe(other.length);
    expect(snapshot?.work(other.length, 50)).toBe(true);
    expect(snapshot?.read({ from: 0, to: other.length }, 'outline')).toMatchObject({ from: 0, to: other.length });
    expect(fake.peek()).toMatchObject({ text: 'a = 1\n', revision: 0, undoDepth: 0 });
  });

  it('given a fake without tree, when the host is read, then it has no treeOf', () => {
    expect(createFakeEditor({ text: 'a' }).host.treeOf).toBeUndefined();
  });

  it('given the tab size configured to 2, when indentUnit is read, then it is two spaces', () => {
    const fake = createFakeEditor({ text: 'a' });

    expect(fake.host.indentUnit?.()).toBe('    ');
    fake.host.configure({ tabSize: 2 });

    expect(fake.host.indentUnit?.()).toBe('  ');
  });
});
