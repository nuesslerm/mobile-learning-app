import { Canvas, Path, Skia } from '@shopify/react-native-skia';
import {
  LoadSkiaWeb,
  WithSkiaWeb,
} from '@shopify/react-native-skia/lib/module/web';
import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { screen } from '../index';

describe('@shopify/react-native-skia stub', () => {
  it('given a Canvas with a Path from Skia.Path.MakeFromSVGString, when it renders, then it exposes data-skia-canvas and the path as data-path', () => {
    const d = 'M 0 0 L 10 10';
    const pathDef = Skia.Path.MakeFromSVGString(d);
    if (!pathDef)
      throw new Error('expected MakeFromSVGString to return a path');
    render(
      <Canvas testID="canvas">
        <Path path={pathDef} />
      </Canvas>
    );
    const canvas = screen.getByTestId('canvas');
    expect(canvas.getAttribute('data-skia-canvas')).not.toBeNull();
    const path = canvas.querySelector('[data-skia="Path"]');
    expect(path?.getAttribute('data-path')).toBe(d);
  });

  it('given a WithSkiaWeb with a fallback and a getComponent, when it renders, then the fallback shows first and the component once loaded', async () => {
    const Fallback = () => <div data-testid="skia-fallback" />;
    const SkiaComponent = () => <div data-testid="skia-loaded" />;
    render(
      <WithSkiaWeb
        fallback={<Fallback />}
        getComponent={() => Promise.resolve({ default: SkiaComponent })}
      />
    );
    expect(
      screen.getByTestId('skia-fallback').getAttribute('data-testid')
    ).toBe('skia-fallback');
    const loaded = await screen.findByTestId('skia-loaded');
    expect(loaded.getAttribute('data-testid')).toBe('skia-loaded');
    expect(screen.queryByTestId('skia-fallback')).toBeNull();
  });

  it('given LoadSkiaWeb, when awaited, then it resolves', async () => {
    await expect(LoadSkiaWeb()).resolves.toBe(true);
  });
});
