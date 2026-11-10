import { describe, expect, it } from 'vitest';

import type { Annotation } from '../src/domain/types';
import { toWireframeMarkdown } from '../src/markdown/toWireframeMarkdown';

function annotation(kind: Annotation['kind'], wireframe: NonNullable<Annotation['wireframe']>): Annotation {
  return {
    id: '1',
    componentName: 'Test',
    note: '',
    platform: 'native',
    kind,
    wireframe,
  };
}

describe('toWireframeMarkdown', () => {
  it('returns empty string when no wireframe annotations exist', () => {
    const out = toWireframeMarkdown([
      { id: '1', componentName: 'Foo', note: '', platform: 'native' },
    ]);
    expect(out).toBe('');
  });

  it('renders placement annotations sorted top-to-bottom', () => {
    const annotations: Annotation[] = [
      annotation('placement', {
        componentType: 'Footer',
        xRatio: 0,
        yRatio: 0.8,
        widthRatio: 1,
        heightRatio: 0.2,
      }),
      annotation('placement', {
        componentType: 'Header',
        xRatio: 0.1,
        yRatio: 0.1,
        widthRatio: 0.8,
        heightRatio: 0.2,
      }),
    ];
    const md = toWireframeMarkdown(annotations, '/storybook', { width: 1000, height: 1000 });
    expect(md).toContain('## Page Feedback: /storybook');
    expect(md).toContain('> 2 components placed');
    expect(md).toContain('Header');
    expect(md).toContain('Footer');
    expect(md).toContain('width: 80.0%');
    expect(md).toContain('Below **Header**');
  });
});
