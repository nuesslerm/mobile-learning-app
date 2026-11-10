import { lazy, Suspense } from 'react';

import { useTheme, XStack, YStack } from '../../primitives';
import { Text } from '../text';
import { View } from '../view';

export type StackedBarChartColor =
  | 'primary'
  | 'accent'
  | 'success'
  | 'warning'
  | 'destructive'
  | 'muted';

export type StackedBarChartSegment = {
  key: string;
  value: number;
  color?: StackedBarChartColor;
};

export type StackedBarChartBar = {
  label: string;
  segments: StackedBarChartSegment[];
};

export type StackedBarChartProps = {
  data: StackedBarChartBar[];
  /** Fires with the owning bar and the pressed segment, for drill-down. */
  onSegmentPress?: (
    bar: StackedBarChartBar,
    segment: StackedBarChartSegment
  ) => void;
  height?: number;
  /** Shown instead of the chart when `data` is empty. */
  emptyLabel?: string;
  testID?: string;
};

const DEFAULT_HEIGHT = 140;

type ThemeToken = { val?: string };
type ThemeShape = Record<string, ThemeToken | undefined>;

// react-native-svg takes plain strings, so semantic names resolve through
// the theme here; every colour still originates from a theme token.
function tokenValue(theme: ThemeShape, name: string): string {
  const token = theme[name];

  return token?.val ?? (theme.transparent as ThemeToken).val ?? 'transparent';
}

function resolveTokenName(
  color: StackedBarChartColor | undefined
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

type StackedBarRow = { label: string } & Record<string, number>;

type VictoryCanvasProps = {
  rows: StackedBarRow[];
  keys: string[];
  colors: string[];
  barWidth: number;
  maxStack: number;
};

// victory-native's dist ships JSX only Metro compiles; a dynamic import keeps
// it (and skia) out of any module graph that imports the barrel without
// rendering the canvas, while vitest still resolves it to a mock when it does.
const VictoryCanvas = lazy(() =>
  import('victory-native').then(({ CartesianChart, StackedBar }) => ({
    default: function VictoryCanvas({
      rows,
      keys,
      colors,
      barWidth,
      maxStack,
    }: VictoryCanvasProps) {
      return (
        <CartesianChart
          data={rows}
          domain={{ y: [0, Math.max(maxStack * 1.12, 1)] }}
          domainPadding={{ left: 16, right: 16, top: 6 }}
          xKey="label"
          yKeys={keys}
        >
          {({ points, chartBounds }) => (
            <StackedBar
              animate={{ type: 'timing' }}
              barWidth={barWidth}
              chartBounds={chartBounds}
              colors={colors}
              points={keys.map((key) => points[key])}
            />
          )}
        </CartesianChart>
      );
    },
  }))
);

function StackedBarChart({
  data,
  onSegmentPress,
  height = DEFAULT_HEIGHT,
  emptyLabel = 'No data yet.',
  testID = 'StackedBarChart',
}: StackedBarChartProps) {
  const theme = useTheme();

  if (data.length === 0) {
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

  // Segments can differ between bars (a missing plan costs nothing on a day it
  // did not run), but victory-native stacks the SAME yKeys across every row, so
  // absent segments are zero-filled exactly like a toggled-off legend series.
  const keys: string[] = [];
  const colorByKey = new Map<string, StackedBarChartColor>();
  for (const bar of data) {
    for (const segment of bar.segments) {
      if (!keys.includes(segment.key)) {
        keys.push(segment.key);
        colorByKey.set(segment.key, segment.color ?? 'primary');
      }
    }
  }

  const rows: StackedBarRow[] = data.map((bar) => {
    const row = { label: bar.label } as StackedBarRow;
    for (const key of keys) {
      const segment = bar.segments.find((s) => s.key === key);
      row[key] = segment?.value ?? 0;
    }
    return row;
  });

  const maxStack = Math.max(
    0,
    ...data.map((bar) =>
      bar.segments.reduce((sum, s) => sum + Math.max(s.value, 0), 0)
    )
  );
  const colors = keys.map((key) =>
    tokenValue(theme, resolveTokenName(colorByKey.get(key)))
  );
  const segmentColor = (color: StackedBarChartColor | undefined): string =>
    tokenValue(theme, resolveTokenName(color));

  return (
    <View overflow="hidden" rounded="$xl" testID={testID}>
      <View height={height} testID={`${testID}-Canvas`}>
        <Suspense fallback={null}>
          <VictoryCanvas
            barWidth={data.length > 14 ? 7 : data.length > 7 ? 14 : 24}
            colors={colors}
            keys={keys}
            maxStack={maxStack}
            rows={rows}
          />
        </Suspense>
      </View>
      <YStack gap="$2" testID={`${testID}-Segments`}>
        {data.map((bar, index) => (
          <YStack key={bar.label} gap="$1" testID={`${testID}-Bar-${index}`}>
            <Text variant="small" testID={`${testID}-Bar-${index}-Label`}>
              {bar.label}
            </Text>
            <XStack flexWrap="wrap" gap="$2">
              {bar.segments.map((segment, segmentIndex) => {
                const id = `${testID}-Bar-${index}-Segment-${segmentIndex}`;
                const row = (
                  <>
                    <YStack
                      width={10}
                      height={10}
                      rounded="$pill"
                      style={{
                        backgroundColor: segmentColor(segment.color),
                      }}
                    />
                    <Text variant="small">{segment.key}</Text>
                  </>
                );
                // The target is the reliable press handle on every platform;
                // hit-testing the Skia bar is left as a bonus, not the contract.
                return onSegmentPress ? (
                  <XStack
                    key={segment.key}
                    items="center"
                    gap="$1"
                    onPress={() => onSegmentPress(bar, segment)}
                    testID={id}
                  >
                    {row}
                  </XStack>
                ) : (
                  <XStack key={segment.key} items="center" gap="$1" testID={id}>
                    {row}
                  </XStack>
                );
              })}
            </XStack>
          </YStack>
        ))}
      </YStack>
    </View>
  );
}

export { StackedBarChart };
