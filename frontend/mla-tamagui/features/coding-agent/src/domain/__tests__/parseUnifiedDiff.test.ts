// features/coding-agent/src/domain/__tests__/parseUnifiedDiff.test.ts
//
// Tests for the pure unified-diff parser. Every test is Given/When/Then and
// keeps the expected shape aligned with the contract:
// `ParsedDiffFile[] {path, oldPath?, additions, deletions, hunks: {header,
// lines: {kind: add|del|ctx, text, oldNo?, newNo?}[]}}`.

import { describe, expect, it } from 'vitest';

import {
  parseUnifiedDiff,
  type ParsedDiffFile,
} from '../diff/parseUnifiedDiff';

describe('parseUnifiedDiff', () => {
  it('given an empty string, when parsed, then an empty array is returned', () => {
    // Given an empty unified-diff text
    const text = '';
    // When parsed
    const files = parseUnifiedDiff(text);
    // Then no files are returned
    expect(files).toEqual([]);
  });

  it('given a two-file patch (modified with two hunks + new file), when parsed, then both files come back with correct counts and old/new numbers', () => {
    // Given a two-file patch: src/math.ts is modified with two hunks, and
    // src/math/new.ts is a brand-new file (--- /dev/null)
    const text = [
      'diff --git a/src/math.ts b/src/math.ts',
      'index 1111111..2222222 100644',
      '--- a/src/math.ts',
      '+++ b/src/math.ts',
      '@@ -1,3 +1,4 @@',
      ' export function add(a: number, b: number) {',
      '-  return a - b;',
      '+  if (typeof a !== "number" || typeof b !== "number") {',
      '+    throw new TypeError("add requires numbers");',
      '   return a + b;',
      ' }',
      '@@ -20,2 +21,2 @@ export function sub(',
      ' sub(a: number, b: number) {',
      '+  return a - b;',
      ' }',
      'diff --git a/src/math/new.ts b/src/math/new.ts',
      'new file mode 100644',
      'index 0000000..3333333',
      '--- /dev/null',
      '+++ b/src/math/new.ts',
      '@@ -0,0 +1,2 @@',
      '+export const PI = 3.14;',
      '+export function area(r: number) { return PI * r * r; }',
      '',
    ].join('\n');

    // When parsed
    const files = parseUnifiedDiff(text);

    // Then two files come back in order with correct shapes
    expect(files).toHaveLength(2);

    const [math, newFile] = files as [ParsedDiffFile, ParsedDiffFile];

    // math.ts: modified, 3 additions and 1 deletion across two hunks
    expect(math.path).toBe('src/math.ts');
    expect(math.oldPath).toBe('src/math.ts');
    expect(math.additions).toBe(3);
    expect(math.deletions).toBe(1);
    expect(math.hunks).toHaveLength(2);
    expect(math.hunks[0]?.header).toBe('@@ -1,3 +1,4 @@');
    expect(math.hunks[1]?.header).toBe(
      '@@ -20,2 +21,2 @@ export function sub('
    );
    expect(math.hunks[0]?.lines[0]).toEqual({
      kind: 'ctx',
      text: 'export function add(a: number, b: number) {',
      oldNo: 1,
      newNo: 1,
    });
    expect(math.hunks[0]?.lines[1]).toEqual({
      kind: 'del',
      text: '  return a - b;',
      oldNo: 2,
    });
    expect(math.hunks[0]?.lines[2]).toEqual({
      kind: 'add',
      text: '  if (typeof a !== "number" || typeof b !== "number") {',
      newNo: 2,
    });
    expect(math.hunks[1]?.lines[1]).toEqual({
      kind: 'add',
      text: '  return a - b;',
      newNo: 22,
    });

    // new.ts: brand-new, 2 additions, no deletions, no oldPath
    expect(newFile.path).toBe('src/math/new.ts');
    expect(newFile.oldPath).toBeUndefined();
    expect(newFile.additions).toBe(2);
    expect(newFile.deletions).toBe(0);
    expect(newFile.hunks).toHaveLength(1);
    expect(newFile.hunks[0]?.lines.map((line) => line.kind)).toEqual([
      'add',
      'add',
    ]);
    expect(newFile.hunks[0]?.lines[0]?.newNo).toBe(1);
    expect(newFile.hunks[0]?.lines[1]?.newNo).toBe(2);
    expect(newFile.hunks[0]?.lines[0]?.oldNo).toBeUndefined();
  });

  it('given a rename (rename from / rename to inside a diff --git), when parsed, then oldPath is preserved and path reflects the new name', () => {
    // Given a rename patch with explicit rename from / rename to lines
    const text = [
      'diff --git a/src/old-name.ts b/src/new-name.ts',
      'similarity index 95%',
      'rename from src/old-name.ts',
      'rename to src/new-name.ts',
      'index 1111111..2222222 100644',
      '--- a/src/old-name.ts',
      '+++ b/src/new-name.ts',
      '@@ -1,2 +1,2 @@',
      '-export const NAME = "old";',
      '+export const NAME = "new";',
      ' export function noop() {}',
      '',
    ].join('\n');

    // When parsed
    const files = parseUnifiedDiff(text);

    // Then exactly one file comes back with the rename preserved
    expect(files).toHaveLength(1);
    const [file] = files as [ParsedDiffFile];
    expect(file.path).toBe('src/new-name.ts');
    expect(file.oldPath).toBe('src/old-name.ts');
    expect(file.additions).toBe(1);
    expect(file.deletions).toBe(1);
    expect(file.hunks[0]?.lines[0]).toMatchObject({
      kind: 'del',
      oldNo: 1,
    });
    expect(file.hunks[0]?.lines[1]).toMatchObject({
      kind: 'add',
      newNo: 1,
    });
  });

  it('given a binary file patch, when parsed, then the file entry has empty hunks and zero counts', () => {
    // Given a binary-file patch with no hunks
    const text = [
      'diff --git a/assets/old.png b/assets/old.png',
      'index 1111111..2222222 100644',
      'Binary files a/assets/old.png and b/assets/old.png differ',
      '',
    ].join('\n');

    // When parsed
    const files = parseUnifiedDiff(text);

    // Then a single file is returned with no hunks and zero counts
    expect(files).toHaveLength(1);
    const [file] = files as [ParsedDiffFile];
    expect(file.path).toBe('assets/old.png');
    expect(file.oldPath).toBe('assets/old.png');
    expect(file.hunks).toEqual([]);
    expect(file.additions).toBe(0);
    expect(file.deletions).toBe(0);
  });

  it('given a diff with a malformed hunk header, when parsed, then the malformed hunk is skipped and the parser does not throw', () => {
    // Given a patch whose hunk header is malformed: numbers are missing
    const text = [
      'diff --git a/src/broken.ts b/src/broken.ts',
      '--- a/src/broken.ts',
      '+++ b/src/broken.ts',
      '@@ not a real hunk header @@',
      ' ctx',
      '+added',
      '-removed',
      'diff --git a/src/after.ts b/src/after.ts',
      '--- a/src/after.ts',
      '+++ b/src/after.ts',
      '@@ -1,1 +1,1 @@',
      '-old',
      '+new',
      '',
    ].join('\n');

    // When parsed
    let files: ParsedDiffFile[] = [];
    let threw = false;
    try {
      files = parseUnifiedDiff(text);
      // eslint-disable-next-line mobile-learn/no-unhandled-catch -- the parser is documented as resilient; any throw here is a real bug and must surface, not be swallowed silently
    } catch {
      threw = true;
    }

    // Then the parser did not throw
    expect(threw).toBe(false);
    // And the file after the malformed hunk still parses cleanly
    expect(files).toHaveLength(1);
    const [after] = files as [ParsedDiffFile];
    expect(after.path).toBe('src/after.ts');
    expect(after.additions).toBe(1);
    expect(after.deletions).toBe(1);
    expect(after.hunks[0]?.lines).toEqual([
      { kind: 'del', text: 'old', oldNo: 1 },
      { kind: 'add', text: 'new', newNo: 1 },
    ]);
  });
});
