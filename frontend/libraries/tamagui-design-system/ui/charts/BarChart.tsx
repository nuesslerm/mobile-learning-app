import { useState } from 'react';
import type { LayoutChangeEvent } from 'react-native';
import Svg, { Line, Rect, Text as SvgText } from 'react-native-svg';

import { useTheme } from '../../primitives';
import { Text } from '../text';
import { View } from '../view';

export type BarChartColor =
  | 'primary'
  | 'destructive'
  | 'success'
  | 'accent'
  | 'warning';

export type BarChartDatum = {
  label: string;
  value: number;
  color?: BarChartColor;
  onPress?: () => void;
};

export type BarChartProps = {
  data: BarChartDatum[];
  height?: number;
  /** Shown instead of the chart when `data` is empty. */
  emptyLabel?: string;
  testID?: string;
};

const DEFAULT_HEIGHT = 140;
const DEFAULT_WIDTH = 280;
const CHART_INSET = 16;
const TOP_INSET = 8;
const LABEL_SPACE = 18;

type ThemeToken = { val?: string };
type ThemeShape = Record<string, ThemeToken | undefined>;

// react-native-svg takes plain strings, so semantic names resolve through
// the theme here; every colour still originates from a theme token.
function tokenValue(theme: ThemeShape, name: string): string {
  const token = theme[name];

  return token?.val ?? (theme.transparent as ThemeToken).val ?? 'transparent';
}

function resolveTokenName(
  color: BarChartColor | undefined
): 'primary' | 'destructive' | 'success' | 'accent' | 'warning' {
  switch (color) {
    case 'destructive':
    case 'success':
    case 'accent':
    case 'warning':
      return color;
    default:
      return 'primary';
  }
}

function BarChart({
  data,
  height = DEFAULT_HEIGHT,
  emptyLabel = 'No data yet.',
  testID = 'BarChart',
}: BarChartProps) {
  const theme = useTheme();
  const [containerWidth, setContainerWidth] = useState(DEFAULT_WIDTH);

  const handleLayout = (event: LayoutChangeEvent) => {
    const { width } = event.nativeEvent.layout;
    if (width > 0) setContainerWidth(width);
  };

  if (data.length === 0) {
    return (
      <View
        height={height}
        items="center"
        justify="center"
        py="$6"
        rounded="$xl"
        onLayout={handleLayout}
        testID={`${testID}-Empty`}
      >
        <Text variant="muted" testID={`${testID}-Empty-Text`}>
          {emptyLabel}
        </Text>
      </View>
    );
  }

  const chartWidth = Math.max(containerWidth - CHART_INSET, 60);
  const chartBottom = height - LABEL_SPACE;
  const drawableHeight = Math.max(chartBottom - TOP_INSET, 1);
  const maxValue = Math.max(0, ...data.map((d) => d.value));
  const scale = drawableHeight / Math.max(maxValue, 1);
  const slot = chartWidth / data.length;
  const barWidth = Math.min(Math.max(slot * 0.6, 4), 48);
  const axisColor = tokenValue(theme, 'border');
  const labelColor = tokenValue(theme, 'muted-foreground');

  return (
    <View
      overflow="hidden"
      rounded="$xl"
      onLayout={handleLayout}
      testID={testID}
    >
      <Svg width={chartWidth} height={height}>
        {data.map((d, index) => {
          const barHeight = Math.max(d.value, 0) * scale;
          const x = index * slot + (slot - barWidth) / 2;
          return (
            <Rect
              key={d.label}
              x={x}
              y={chartBottom - barHeight}
              width={barWidth}
              height={barHeight}
              rx={6}
              fill={tokenValue(theme, resolveTokenName(d.color))}
              onPress={d.onPress}
              testID={`${testID}-Bar-${index}`}
            />
          );
        })}
        <Line
          x1={0}
          y1={chartBottom}
          x2={chartWidth}
          y2={chartBottom}
          stroke={axisColor}
          strokeWidth={1}
        />
        {data.map((d, index) => (
          <SvgText
            key={`${d.label}-label`}
            x={index * slot + slot / 2}
            y={height - 4}
            fontSize={11}
            fill={labelColor}
            textAnchor="middle"
          >
            {d.label}
          </SvgText>
        ))}
      </Svg>
    </View>
  );
}

export { BarChart };
