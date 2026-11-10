import { lazy, Suspense, useCallback, useLayoutEffect, useState } from 'react';

import { useTheme, XStack, YStack } from '../../primitives';
import { Text } from '../text';
import { View } from '../view';

export type TimeSeriesChartColor =
  | 'primary'
  | 'accent'
  | 'success'
  | 'warning'
  | 'destructive'
  | 'muted';

export type TimeSeriesPoint = { t: number; v: number };

export type TimeSeries = {
  key: string;
  points: TimeSeriesPoint[];
  color?: TimeSeriesChartColor;
};

export type TimeSeriesThreshold = {
  value: number;
  label: string;
};

export type TimeSeriesChartProps = {
  series: TimeSeries[];
  /** A horizontal rule at `value` with `label` drawn inside the plot area. */
  threshold?: TimeSeriesThreshold;
  height?: number;
  /** Shown instead of the chart when `series` is empty. */
  emptyLabel?: string;
  testID?: string;
};

const DEFAULT_HEIGHT = 160;

type ThemeToken = { val?: string };
type ThemeShape = Record<string, ThemeToken | undefined>;

// react-native-svg / Skia take plain strings, so semantic names resolve
// through the theme here; every colour still originates from a theme token.
function tokenValue(theme: ThemeShape, name: string): string {
  const token = theme[name];

  return token?.val ?? (theme.transparent as ThemeToken).val ?? 'transparent';
}

function resolveTokenName(
  color: TimeSeriesChartColor | undefined
): 'primary' | 'accent' | 'success' | 'warning' | 'destructive' | 'muted' {
  switch (color) {
    case 'primary':
    case 'accent':
    case 'success':
    case 'warning':
    case 'destructive':
    case 'muted':
      return color;
    default:
      return 'primary';
  }
}

type TimeSeriesRow = { t: number } & Record<string, number | null>;

type ChartGeometry = {
  left: number;
  right: number;
  top: number;
  bottom: number;
  thresholdY: number | null;
};

function thresholdLabelTop(
  thresholdY: number,
  top: number,
  bottom: number
): number {
  // Keep the pill on the line but never clipped by the plot edges: below the
  // rule near the top, above it near the bottom, centred otherwise.
  if (thresholdY < top + 16) {
    return thresholdY + 6;
  }
  if (thresholdY > bottom - 20) {
    return thresholdY - 24;
  }
  return thresholdY - 9;
}

type GeometryReporterProps = {
  chartBounds: { left: number; right: number; top: number; bottom: number };
  thresholdY: number | null;
  onReport: (geometry: ChartGeometry) => void;
};

// The label lives outside the Skia canvas, so the threshold's drawn y has to
// be reported out of the render prop. A layout effect reports once per
// distinct geometry instead of on every canvas render.
function GeometryReporter({
  chartBounds,
  thresholdY,
  onReport,
}: GeometryReporterProps) {
  useLayoutEffect(() => {
    onReport({
      left: chartBounds.left,
      right: chartBounds.right,
      top: chartBounds.top,
      bottom: chartBounds.bottom,
      thresholdY,
    });
  }, [
    chartBounds.bottom,
    chartBounds.left,
    chartBounds.right,
    chartBounds.top,
    onReport,
    thresholdY,
  ]);

  return null;
}

type VictoryCanvasProps = {
  rows: TimeSeriesRow[];
  keys: string[];
  colors: string[];
  maxValue: number;
  thresholdValue: number | null;
  thresholdColor: string;
  onReportGeometry: (geometry: ChartGeometry) => void;
};

// victory-native's dist ships JSX only Metro compiles; a dynamic import keeps
// it (and Skia) out of any module graph that imports the barrel without
// rendering the canvas, while vitest still resolves it to a mock when it does.
const VictoryCanvas = lazy(() =>
  Promise.all([
    import('victory-native'),
    import('@shopify/react-native-skia'),
  ]).then(([{ CartesianChart, Line }, { DashPathEffect }]) => ({
    default: function VictoryCanvas({
      rows,
      keys,
      colors,
      maxValue,
      thresholdValue,
      thresholdColor,
      onReportGeometry,
    }: VictoryCanvasProps) {
      return (
        <CartesianChart
          data={rows}
          domain={{ y: [0, Math.max(maxValue * 1.12, 1)] }}
          domainPadding={{ left: 12, right: 12, top: 12, bottom: 6 }}
          xKey="t"
          yKeys={keys}
        >
          {({ points, chartBounds, yScale }) => {
            const thresholdY =
              thresholdValue == null ? null : yScale(thresholdValue);

            return (
              <>
                {keys.map((key, index) => (
                  <Line
                    key={key}
                    color={colors[index]}
                    points={points[key]}
                    strokeWidth={2}
                  />
                ))}
                {thresholdY != null && (
                  <Line
                    color={thresholdColor}
                    points={[
                      {
                        x: chartBounds.left,
                        xValue: 0,
                        y: thresholdY,
                        yValue: thresholdValue,
                      },
                      {
                        x: chartBounds.right,
                        xValue: 1,
                        y: thresholdY,
                        yValue: thresholdValue,
                      },
                    ]}
                    strokeWidth={1}
                  >
                    <DashPathEffect intervals={[6, 5]} />
                  </Line>
                )}
                <GeometryReporter
                  chartBounds={chartBounds}
                  onReport={onReportGeometry}
                  thresholdY={thresholdY}
                />
              </>
            );
          }}
        </CartesianChart>
      );
    },
  }))
);

function TimeSeriesChart({
  series,
  threshold,
  height = DEFAULT_HEIGHT,
  emptyLabel = 'No data yet.',
  testID = 'TimeSeriesChart',
}: TimeSeriesChartProps) {
  const theme = useTheme();
  const [geometry, setGeometry] = useState<ChartGeometry | null>(null);

  const reportGeometry = useCallback((next: ChartGeometry) => {
    setGeometry((previous) => {
      if (
        previous?.left === next.left &&
        previous.right === next.right &&
        previous.top === next.top &&
        previous.bottom === next.bottom &&
        previous.thresholdY === next.thresholdY
      ) {
        return previous;
      }
      return next;
    });
  }, []);

  if (series.length === 0 || series.every((s) => s.points.length === 0)) {
    return (
      <View
        height={height}
        items="center"
        justify="center"
        py="$6"
        rounded="$xl"
        testID={`${testID}-Empty`}
      >
        <Text variant="muted" testID={`${testID}-Empty-Text`}>
          {emptyLabel}
        </Text>
      </View>
    );
  }

  // Series share a time axis, so every timestamp becomes one row; a series
  // that has no sample at that timestamp gets null and its line breaks there
  // instead of being drawn across the gap.
  const timestamps = [
    ...new Set(series.flatMap((s) => s.points.map((p) => p.t))),
  ].sort((a, b) => a - b);
  const rows: TimeSeriesRow[] = timestamps.map((t) => {
    const row: TimeSeriesRow = { t };
    for (const s of series) {
      const point = s.points.find((p) => p.t === t);
      row[s.key] = point == null ? null : point.v;
    }
    return row;
  });

  const keys = series.map((s) => s.key);
  const colors = keys.map((key, index) =>
    tokenValue(theme, resolveTokenName(series[index].color))
  );
  const maxValue = Math.max(
    0,
    ...series.flatMap((s) => s.points.map((p) => p.v)),
    threshold?.value ?? 0
  );
  const seriesColor = (color: TimeSeriesChartColor | undefined): string =>
    tokenValue(theme, resolveTokenName(color));

  return (
    <View overflow="hidden" rounded="$xl" testID={testID}>
      <View height={height} position="relative" testID={`${testID}-Canvas`}>
        <Suspense fallback={null}>
          <VictoryCanvas
            colors={colors}
            keys={keys}
            maxValue={maxValue}
            onReportGeometry={reportGeometry}
            rows={rows}
            thresholdColor={tokenValue(theme, 'destructive')}
            thresholdValue={threshold?.value ?? null}
          />
        </Suspense>
        {threshold != null && geometry?.thresholdY != null && (
          <View
            position="absolute"
            t={thresholdLabelTop(
              geometry.thresholdY,
              geometry.top,
              geometry.bottom
            )}
            r={8}
            px="$2"
            py="$0.5"
            rounded="$4"
            bg="$background"
            borderColor="$borderColor"
            borderWidth={1}
            testID={`${testID}-Threshold-Label`}
          >
            <Text color="$muted-foreground" variant="small">
              {threshold.label}
            </Text>
          </View>
        )}
      </View>
      <YStack gap="$2" testID={`${testID}-Series`}>
        {series.map((s, index) => (
          <XStack
            key={s.key}
            items="center"
            gap="$1"
            testID={`${testID}-Series-${index}`}
          >
            <YStack
              width={10}
              height={10}
              rounded="$pill"
              style={{ backgroundColor: seriesColor(s.color) }}
            />
            <Text variant="small">{s.key}</Text>
          </XStack>
        ))}
      </YStack>
    </View>
  );
}

export { TimeSeriesChart };
