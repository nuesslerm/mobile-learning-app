import { useState } from 'react';
import type { LayoutChangeEvent } from 'react-native';
import Svg, { Circle, G, Line, Rect, Text as SvgText } from 'react-native-svg';

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
  variant?: 'standard' | 'distribution';
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
  variant = 'standard',
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

  const distribution = variant === 'distribution';
  const axisInset = distribution ? 32 : 0;
  const chartWidth = Math.max(containerWidth - (distribution ? 0 : CHART_INSET), 60);
  const chartBottom = height - LABEL_SPACE;
  const drawableHeight = Math.max(chartBottom - (distribution ? 18 : TOP_INSET), 1);
  const maxValue = Math.max(0, ...data.map((d) => d.value));
  const scale = drawableHeight / Math.max(maxValue, distribution ? 100 : 1);
  const slot = (chartWidth - axisInset) / data.length;
  const barWidth = distribution ? 3 : Math.min(Math.max(slot * 0.6, 4), 48);
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
        {distribution && [25, 50, 75, 100].map((percentage) => {
          const y = chartBottom - drawableHeight * percentage / 100;
          return (
            <G key={percentage}>
              <Line x1={axisInset} x2={chartWidth} y1={y} y2={y} stroke={axisColor} strokeWidth={1} opacity={0.45} />
              <SvgText x={4} y={y + 4} fontSize={10} fill={labelColor}>{percentage}%</SvgText>
            </G>
          );
        })}
        {data.map((d, index) => {
          const barHeight = Math.max(d.value, 0) * scale;
          const x = axisInset + index * slot + (slot - barWidth) / 2;
          const color = tokenValue(theme, resolveTokenName(d.color));
          return (
            <G key={d.label}>
              <Rect
                x={x}
                y={chartBottom - barHeight}
                width={barWidth}
                height={barHeight}
                rx={distribution ? 0 : 6}
                fill={color}
                onPress={d.onPress}
                testID={`${testID}-Bar-${index}`}
              />
              {distribution && d.value === maxValue && <Circle cx={x + barWidth / 2} cy={chartBottom - barHeight} r={7}
                fill={color} stroke={labelColor} strokeWidth={1} />}
            </G>
          );
        })}
        <Line
          x1={axisInset}
          y1={chartBottom}
          x2={chartWidth}
          y2={chartBottom}
          stroke={axisColor}
          strokeWidth={1}
        />
        {data.map((d, index) => (
          <SvgText
            key={`${d.label}-label`}
            x={axisInset + index * slot + slot / 2}
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
