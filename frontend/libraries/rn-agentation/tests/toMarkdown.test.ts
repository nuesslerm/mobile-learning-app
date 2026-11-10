import { describe, expect, it } from 'vitest';

import type { Annotation } from '../src/domain/types';
import { isUserComponentName, parseComponentName } from '../src/domain/types';
import { toMarkdown } from '../src/markdown/toMarkdown';

const base: Annotation = {
  id: '1',
  componentName: 'PrimaryButton',
  note: 'spacing is too tight',
  platform: 'web',
  source: { fileName: 'app/(tabs)/index.tsx', lineNumber: 42, columnNumber: 7 },
  rect: { x: 10.4, y: 20.6, width: 100, height: 48 },
  props: { title: 'Go', disabled: false },
};

describe('toMarkdown', () => {
  it('renders an empty-state when there are no annotations', () => {
    const md = toMarkdown([]);
    expect(md).toContain('No annotations captured yet');
  });

  it('includes the real file:line as the primary locator', () => {
    const md = toMarkdown([base]);
    expect(md).toContain('`app/(tabs)/index.tsx:42:7`');
    expect(md).toContain('PrimaryButton');
    expect(md).toContain('spacing is too tight');
  });

  it('rounds bounds and serializes props', () => {
    const md = toMarkdown([base]);
    expect(md).toContain('x=10 y=21 w=100 h=48');
    expect(md).toContain('"title": "Go"');
  });

  it('falls back gracefully when source is missing', () => {
    const md = toMarkdown([{ ...base, source: undefined }]);
    expect(md).toContain('unavailable');
  });

  it('drops functions from props instead of throwing', () => {
    const md = toMarkdown([{ ...base, props: { onPress: () => {} } }]);
    expect(md).toContain('[Function]');
  });

  it('renders the component owner chain when present', () => {
    const md = toMarkdown([
      { ...base, source: undefined, ownerChain: ['ProblemRow', 'ProblemListScreen'] },
    ]);
    expect(md).toContain('Component chain:');
    expect(md).toContain('ProblemRow › ProblemListScreen');
  });

  it('renders sourceFile when an exact line is unavailable (React 19)', () => {
    const md = toMarkdown([
      { ...base, source: undefined, sourceFile: '(tabs)/index.tsx' },
    ]);
    expect(md).toContain('Source file:');
    expect(md).toContain('(tabs)/index.tsx');
  });

  it('numbers multiple annotations', () => {
    const md = toMarkdown([base, { ...base, id: '2', componentName: 'Card' }]);
    expect(md).toContain('## 1. PrimaryButton');
    expect(md).toContain('## 2. Card');
  });
});

describe('parseComponentName', () => {
  it('splits a React 19 display name with an embedded file', () => {
    expect(parseComponentName('HomeScreen(./(tabs)/index.tsx)')).toEqual({
      name: 'HomeScreen',
      fileName: '(tabs)/index.tsx',
    });
  });

  it('returns the bare name when there is no file', () => {
    expect(parseComponentName('ProblemRow')).toEqual({ name: 'ProblemRow' });
  });

  it('handles undefined', () => {
    expect(parseComponentName(undefined)).toEqual({ name: 'unknown' });
  });
});

describe('isUserComponentName', () => {
  it('accepts user components, rejects host/noise', () => {
    expect(isUserComponentName('ProblemRow')).toBe(true);
    expect(isUserComponentName('View')).toBe(false);
    expect(isUserComponentName('div')).toBe(false);
    expect(isUserComponentName('CssInterop.View')).toBe(false);
    expect(isUserComponentName('ForwardRef')).toBe(false);
  });
});
