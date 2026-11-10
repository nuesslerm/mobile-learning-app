// @vitest-environment jsdom
import type { ReactElement } from 'react';
import { act } from 'react';
import type { Root } from 'react-dom/client';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { TamaguiProvider } from '../../../primitives';

// victory-native renders through @shopify/react-native-skia, which has no
// runtime in jsdom. The chart's own logic under test -- series wiring, the
// threshold label overlay, the empty state -- lives outside the canvas, so the
// canvas is stubbed with inert geometry and the real component still runs.
vi.mock('victory-native', () => {
  return {
    CartesianChart: ({
      children,
    }: {
      children: (args: {
        points: Record<string, unknown>;
        chartBounds: {
          left: number;
          right: number;
          top: number;
          bottom: number;
        };
        yScale: (value: number) => number;
      }) => unknown;
    }) =>
      children({
        points: {},
        chartBounds: { left: 0, right: 300, top: 0, bottom: 200 },
        yScale: () => 80,
      }),
    Line: ({ color }: { color?: string }) => (
      <div data-testid="chart-line" data-color={color} />
    ),
  };
});

vi.mock('@shopify/react-native-skia', () => {
  return {
    DashPathEffect: () => null,
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
import { TimeSeriesChart, type TimeSeries } from '../TimeSeriesChart';

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

const TWO_SERIES: TimeSeries[] = [
  {
    key: 'vm-gateway',
    points: [
      { t: 1789000000, v: 12.4 },
      { t: 1789000060, v: 14.1 },
    ],
    color: 'accent',
  },
  {
    key: 'vm-ops-api',
    points: [
      { t: 1789000000, v: 3.2 },
      { t: 1789000060, v: 4.0 },
    ],
    color: 'success',
  },
];

describe('given a TimeSeriesChart with two series', () => {
  it('when rendered, then every series renders its legend row with a distinct colour', async () => {
    const container = await render(
      <TimeSeriesChart series={TWO_SERIES} testID="Chart" />
    );

    const row0 = container.querySelector('[data-testid="Chart-Series-0"]');
    const row1 = container.querySelector('[data-testid="Chart-Series-1"]');
    expect(row0?.textContent).toContain('vm-gateway');
    expect(row1?.textContent).toContain('vm-ops-api');

    const swatch0 = row0?.firstElementChild as HTMLElement;
    const swatch1 = row1?.firstElementChild as HTMLElement;
    expect(swatch0.style.backgroundColor).not.toBe('');
    expect(swatch0.style.backgroundColor).not.toBe(
      swatch1.style.backgroundColor
    );

    const lineColors = Array.from(
      container.querySelectorAll('[data-testid="chart-line"]')
    ).map((line) => line.getAttribute('data-color'));
    expect(lineColors).toHaveLength(2);
    expect(lineColors[0]).not.toBe(lineColors[1]);
  });
});

describe('given a rendered chart with a threshold', () => {
  it('when rendered, then the threshold rule and its label are drawn', async () => {
    const container = await render(
      <TimeSeriesChart
        series={TWO_SERIES}
        threshold={{ value: 100, label: '2 OCPU' }}
        testID="Chart"
      />
    );

    const label = container.querySelector(
      '[data-testid="Chart-Threshold-Label"]'
    );
    expect(label?.textContent).toContain('2 OCPU');
    expect(
      container.querySelectorAll('[data-testid="chart-line"]')
    ).toHaveLength(3);
  });
});

describe('given an empty series set', () => {
  it('when rendered, then the emptyLabel is shown instead of the canvas', async () => {
    const container = await render(
      <TimeSeriesChart
        series={[]}
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
