import { View as TamaguiView } from '@lib/tamagui-design-system';
import { render } from '@testing-library/react';
import { useState } from 'react';
import { Text, View } from 'react-native';
import { describe, expect, it } from 'vitest';

import { renderScreen, screen, waitFor } from '../index';
import { resetLayouts, setLayout } from '../interactions/layout';

function TamaguiLayoutProbe() {
  const [box, setBox] = useState('none');
  return (
    <TamaguiView
      testID="tamagui-probe"
      onLayout={(event) =>
        setBox(
          `${event.nativeEvent.layout.width}x${event.nativeEvent.layout.height}`
        )
      }
    >
      <Text testID="tamagui-probe-text">{box}</Text>
    </TamaguiView>
  );
}

function LayoutProbe() {
  const [box, setBox] = useState('none');
  return (
    <View
      testID="probe"
      onLayout={(event) =>
        setBox(
          `${event.nativeEvent.layout.width}x${event.nativeEvent.layout.height}`
        )
      }
    >
      <Text testID="probe-text">{box}</Text>
    </View>
  );
}

describe('layout driver', () => {
  it('given a View with onLayout, when setLayout gives it a box, then onLayout reports width x height', async () => {
    render(<LayoutProbe />);
    const el = screen.getByTestId('probe');

    setLayout(el, { width: 320, height: 200 });

    await waitFor(() =>
      expect(screen.getByTestId('probe-text').textContent).toBe('320x200')
    );
  });

  it('given a box then a reset, when the observers re-fire, then onLayout reads the default 0x0 box', async () => {
    render(<LayoutProbe />);
    const el = screen.getByTestId('probe');

    setLayout(el, { width: 320, height: 200 });
    await waitFor(() =>
      expect(screen.getByTestId('probe-text').textContent).toBe('320x200')
    );

    resetLayouts();

    await waitFor(() =>
      expect(screen.getByTestId('probe-text').textContent).toBe('0x0')
    );
  });

  it('given an element with a box, when an IntersectionObserver observes it, then the entry carries that box', async () => {
    render(<View testID="observed" />);
    const el = screen.getByTestId('observed');
    setLayout(el, { x: 10, y: 20, width: 320, height: 200 });

    const entries = await new Promise<IntersectionObserverEntry[]>((resolve) => {
      new IntersectionObserver((delivered) => resolve(delivered)).observe(el);
    });

    expect(entries).toHaveLength(1);
    expect(entries[0].target).toBe(el);
    expect(entries[0].boundingClientRect).toMatchObject({ x: 10, y: 20, width: 320, height: 200 });
    expect(entries[0].isIntersecting).toBe(true);
  });

  it('given a Tamagui View already mounted, when setLayout gives it a box, then its onLayout reports width x height', async () => {
    renderScreen(<TamaguiLayoutProbe />);
    const el = screen.getByTestId('tamagui-probe');

    setLayout(el, { width: 320, height: 200 });

    await waitFor(
      () =>
        expect(screen.getByTestId('tamagui-probe-text').textContent).toBe('320x200'),
      { timeout: 3000 }
    );
  });
});
