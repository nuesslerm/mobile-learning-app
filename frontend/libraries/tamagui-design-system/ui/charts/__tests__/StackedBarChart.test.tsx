// @vitest-environment jsdom
import type { ReactElement } from 'react';
import { act } from 'react';
import type { Root } from 'react-dom/client';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { TamaguiProvider } from '../../../primitives';

// victory-native renders through @shopify/react-native-skia, which has no
// runtime in jsdom. The chart's own logic under test -- per-bar segment
// targets, press wiring, the empty state -- lives outside the canvas, so the
// canvas is stubbed with inert geometry and the real component still runs.
vi.mock('victory-native', () => {
  return {
    CartesianChart: ({ children }: { children: (args: unknown) => unknown }) =>
      children({
        points: {},
        chartBounds: { left: 0, right: 300, top: 0, bottom: 200 },
      }),
    StackedBar: () => null,
  };
});

vi.hoisted(() => {
  (
    globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  const w = globalThis.window as unknown as Record<string, unknown>;
  if (typeof w.matchMedia !== 'function') {
    w.matchMedia = (query: string) =>
      ({
        matches: false,
        media: query,
        onchange: null,
        addListener: () => undefined,
        removeListener: () => undefined,
        addEventListener: () => undefined,
        removeEventListener: () => undefined,
        dispatchEvent: () => false,
      }) as unknown as MediaQueryList;
  }
  if (typeof w.ResizeObserver !== 'function') {
    class ResizeObserverStub {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    }
    w.ResizeObserver = ResizeObserverStub;
  }
});

import { tamaguiConfig } from '../../../theme/tamagui.config';
import {
  StackedBarChart,
  type StackedBarChartBar,
  type StackedBarChartSegment,
} from '../StackedBarChart';

const renderedRoots: Root[] = [];

async function render(element: ReactElement) {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root = createRoot(container);
  renderedRoots.push(root);
  await act(async () => {
    root.render(
      <TamaguiProvider config={tamaguiConfig} defaultTheme="light">
        {element}
      </TamaguiProvider>
    );
  });
  return container;
}

afterEach(() => {
  for (const root of renderedRoots.splice(0)) {
    act(() => root.unmount());
  }
  document.body.innerHTML = '';
});

function segmentTarget(
  container: HTMLElement,
  testID: string,
  bar: number,
  segment: number
): HTMLElement {
  const el = container.querySelector<HTMLElement>(
    `[data-testid="${testID}-Bar-${bar}-Segment-${segment}"]`
  );
  if (!el)
    throw new Error(`no segment target for bar ${bar} segment ${segment}`);
  return el;
}

function press(element: HTMLElement): void {
  act(() => {
    element.dispatchEvent(
      new MouseEvent('click', { bubbles: true, cancelable: true })
    );
  });
}

const TWO_BARS: StackedBarChartBar[] = [
  {
    label: '09-12',
    segments: [
      { key: 'settings-screen', value: 0.22, color: 'primary' },
      { key: 'dashboards', value: 0.31, color: 'accent' },
    ],
  },
  {
    label: '09-13',
    segments: [
      { key: 'settings-screen', value: 0.12, color: 'primary' },
      { key: 'dashboards', value: 0.41, color: 'accent' },
      { key: 'harness', value: 0.18, color: 'success' },
    ],
  },
];

describe('given a StackedBarChart with two bars of stacked segments', () => {
  it('when rendered, then every bar shows one press target per segment', async () => {
    const container = await render(
      <StackedBarChart data={TWO_BARS} testID="Chart" />
    );

    expect(segmentTarget(container, 'Chart', 0, 0).textContent).toContain(
      'settings-screen'
    );
    expect(segmentTarget(container, 'Chart', 0, 1).textContent).toContain(
      'dashboards'
    );
    expect(segmentTarget(container, 'Chart', 1, 0).textContent).toContain(
      'settings-screen'
    );
    expect(segmentTarget(container, 'Chart', 1, 1).textContent).toContain(
      'dashboards'
    );
    expect(segmentTarget(container, 'Chart', 1, 2).textContent).toContain(
      'harness'
    );
  });
});

describe('given a rendered chart with a segment press handler', () => {
  it('when a segment target is pressed, then onSegmentPress fires with that bar and that segment', async () => {
    const pressed: {
      bar: StackedBarChartBar;
      segment: StackedBarChartSegment;
    }[] = [];
    const container = await render(
      <StackedBarChart
        data={TWO_BARS}
        onSegmentPress={(bar, segment) => pressed.push({ bar, segment })}
        testID="Chart"
      />
    );

    const target = segmentTarget(container, 'Chart', 1, 2);
    press(target);

    expect(pressed).toHaveLength(1);
    expect(pressed[0]).toEqual({
      bar: TWO_BARS[1],
      segment: TWO_BARS[1].segments[2],
    });
  });
});

describe('given an empty data set', () => {
  it('when rendered, then the emptyLabel is shown instead of the canvas', async () => {
    const container = await render(
      <StackedBarChart
        data={[]}
        emptyLabel="Nothing to chart."
        testID="Chart"
      />
    );

    const empty = container.querySelector('[data-testid="Chart-Empty"]');
    expect(empty).not.toBeNull();
    expect(empty?.textContent).toBe('Nothing to chart.');
    expect(container.querySelector('[data-testid="Chart-Canvas"]')).toBeNull();
  });
});
