import { useState } from 'react';
import type { LayoutChangeEvent } from 'react-native';
import Svg, { Circle, Line, Path, Text as SvgText } from 'react-native-svg';

import { useTheme } from '../../primitives';
import { Text } from '../text';
import { View } from '../view';

export type LineChartDatum = {
  value: number;
  label?: string;
  onPress?: () => void;
};

export type LineChartColor = 'primary' | 'destructive' | 'success' | 'accent';

export type LineChartProps = {
  data: LineChartDatum[];
  /** Semantic color, resolved to a real color via the theme. Default 'primary'. */
  color?: LineChartColor;
  height?: number;
  /** Shown instead of the chart when `data` is empty. */
  emptyLabel?: string;
  testID?: string;
};

const DEFAULT_HEIGHT = 100;
const DEFAULT_WIDTH = 280;
const CHART_INSET = 16;
const PAD_X = 8;
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
  color: LineChartColor
): 'primary' | 'destructive' | 'success' | 'accent' {
  switch (color) {
    case 'destructive':
    case 'success':
    case 'accent':
      return color;
    default:
      return 'primary';
  }
}

function LineChart({
  data,
  color = 'primary',
  height = DEFAULT_HEIGHT,
  emptyLabel = 'No data yet.',
  testID = 'LineChart',
}: LineChartProps) {
  const theme = useTheme();
  const [containerWidth, setContainerWidth] = useState(DEFAULT_WIDTH);

  const handleLayout = (event: LayoutChangeEvent) => {
    const { width } = event.nativeEvent.layout;
    if (width > 0) setContainerWidth(width);
  };

  // Without data there is no range to scale against; render the empty state
  // rather than dividing by a zero span.
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
  const values = data.map((d) => d.value);
  const minValue = Math.min(...values);
  const span = Math.max(...values) - minValue;

  const xAt = (index: number) =>
    PAD_X +
    (data.length > 1
      ? (index * (chartWidth - PAD_X * 2)) / (data.length - 1)
      : chartWidth / 2);
  const yAt = (value: number) =>
    TOP_INSET +
    (span > 0
      ? (1 - (value - minValue) / span) * drawableHeight
      : drawableHeight / 2);

  const resolvedColor = tokenValue(theme, resolveTokenName(color));
  const axisColor = tokenValue(theme, 'border');
  const labelColor = tokenValue(theme, 'muted-foreground');
  const path = data
    .map(
      (d, index) => `${index === 0 ? 'M' : 'L'} ${xAt(index)} ${yAt(d.value)}`
    )
    .join(' ');

  return (
    <View
      overflow="hidden"
      rounded="$xl"
      onLayout={handleLayout}
      testID={testID}
    >
      <Svg width={chartWidth} height={height}>
        <Path d={path} stroke={resolvedColor} strokeWidth={2} fill="none" />
        {data.map((d, index) => (
          <Circle
            key={`point-${index}`}
            cx={xAt(index)}
            cy={yAt(d.value)}
            r={3}
            fill={resolvedColor}
            onPress={d.onPress}
          />
        ))}
        <Line
          x1={0}
          y1={chartBottom}
          x2={chartWidth}
          y2={chartBottom}
          stroke={axisColor}
          strokeWidth={1}
        />
        {data.map((d, index) =>
          d.label == null ? null : (
            <SvgText
              key={`label-${index}`}
              x={xAt(index)}
              y={height - 4}
              fontSize={11}
              fill={labelColor}
              textAnchor="middle"
            >
              {d.label}
            </SvgText>
          )
        )}
      </Svg>
    </View>
  );
}

export { LineChart };
