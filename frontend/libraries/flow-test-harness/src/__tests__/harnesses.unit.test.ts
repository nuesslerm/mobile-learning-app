import { router } from 'expo-router';
import { GlassContainer, GlassView, isGlassEffectAPIAvailable, isLiquidGlassAvailable } from 'expo-glass-effect';
import { createElement } from 'react';
import { describe, expect, it } from 'vitest';

import { resetLiquidGlass, routerHarness, screen, setLiquidGlass, storageHarness } from '../index';
import { renderScreen } from '../render';

describe('harnesses', () => {
  it('given router pushes and a storage seed, when read back and reset, then both handles reflect the stub state', () => {
    const r = routerHarness();
    const s = storageHarness();
    router.push('/a');
    router.push('/b');
    s.seed({ token: 'abc' });
    expect(r.pushes()).toEqual(['/a', '/b']);
    expect(s.get('token')).toBe('abc');
    r.reset();
    s.reset();
    expect(r.calls()).toEqual([]);
  });
});

describe('liquid glass switch', () => {
  it('given the default, when availability is read, then both checks are false and a GlassView is a plain view', () => {
    renderScreen(createElement(GlassView, { testID: 'glass' }));

    expect(isLiquidGlassAvailable()).toBe(false);
    expect(isGlassEffectAPIAvailable()).toBe(false);
    expect(screen.getByTestId('glass').hasAttribute('data-glass-style')).toBe(false);
  });

  it('given setLiquidGlass(true), when availability is read, then both are true and GlassView and GlassContainer carry their data', () => {
    setLiquidGlass(true);

    renderScreen(
      createElement(
        GlassContainer,
        { testID: 'container', spacing: 12 },
        createElement(GlassView, { testID: 'plain' }),
        createElement(GlassView, { testID: 'clear', glassEffectStyle: { style: 'clear' }, tintColor: '#123456', isInteractive: true, colorScheme: 'dark' })
      )
    );

    expect(isLiquidGlassAvailable()).toBe(true);
    expect(isGlassEffectAPIAvailable()).toBe(true);
    expect(screen.getByTestId('plain').getAttribute('data-glass-style')).toBe('regular');
    expect(screen.getByTestId('plain').getAttribute('data-glass-interactive')).toBe('false');
    expect(screen.getByTestId('clear').getAttribute('data-glass-style')).toBe('clear');
    expect(screen.getByTestId('clear').getAttribute('data-glass-tint')).toBe('#123456');
    expect(screen.getByTestId('clear').getAttribute('data-glass-interactive')).toBe('true');
    expect(screen.getByTestId('clear').getAttribute('data-glass-color-scheme')).toBe('dark');
    expect(screen.getByTestId('container').getAttribute('data-glass-spacing')).toBe('12');
  });

  it('given the previous test turned the switch on, when the next test starts, then it is off again', () => {
    expect(isLiquidGlassAvailable()).toBe(false);
    expect(isGlassEffectAPIAvailable()).toBe(false);
  });

  it('given the switch is on, when it is reset, then both checks are false', () => {
    setLiquidGlass(true);

    resetLiquidGlass();

    expect(isLiquidGlassAvailable()).toBe(false);
  });
});
