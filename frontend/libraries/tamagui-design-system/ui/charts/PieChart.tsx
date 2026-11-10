import { Pressable } from 'react-native';
import Svg, { Circle, G, Path, Text as SvgText } from 'react-native-svg';

import { useTheme, XStack, YStack } from '../../primitives';
import { Text } from '../text';
import { View } from '../view';

export type PieChartColor =
  | 'primary'
  | 'destructive'
  | 'success'
  | 'accent'
  | 'warning'
  | 'muted';

export type PieChartDatum = {
  label: string;
  value: number;
  color?: PieChartColor;
  onPress?: () => void;
};

export type PieChartProps = {
  data: PieChartDatum[];
  radius?: number;
  /** Shown instead of the chart when every value is zero or the list is empty. */
  emptyLabel?: string;
  testID?: string;
};

const DEFAULT_RADIUS = 72;

type ThemeToken = { val?: string };
type ThemeShape = Record<string, ThemeToken | undefined>;

// react-native-svg takes plain strings, so semantic names resolve through
// the theme here; every colour still originates from a theme token.
function tokenValue(theme: ThemeShape, name: string): string {
  const token = theme[name];

  return token?.val ?? (theme.transparent as ThemeToken).val ?? 'transparent';
}

function resolveTokenName(
  color: PieChartColor | undefined
): 'primary' | 'destructive' | 'success' | 'accent' | 'warning' | 'muted' {
  switch (color) {
    case 'primary':
    case 'destructive':
    case 'success':
    case 'accent':
    case 'warning':
    case 'muted':
      return color;
    default:
      return 'primary';
  }
}

function polar(radius: number, angle: number) {
  return { x: radius * Math.cos(angle), y: radius * Math.sin(angle) };
}

function slicePath(
  radius: number,
  innerRadius: number,
  startAngle: number,
  endAngle: number
): string {
  const largeArc = endAngle - startAngle > Math.PI ? 1 : 0;
  const outerStart = polar(radius, startAngle);
  const outerEnd = polar(radius, endAngle);
  const innerEnd = polar(innerRadius, endAngle);
  const innerStart = polar(innerRadius, startAngle);
  return [
    `M ${outerStart.x} ${outerStart.y}`,
    `A ${radius} ${radius} 0 ${largeArc} 1 ${outerEnd.x} ${outerEnd.y}`,
    `L ${innerEnd.x} ${innerEnd.y}`,
    `A ${innerRadius} ${innerRadius} 0 ${largeArc} 0 ${innerStart.x} ${innerStart.y}`,
    'Z',
  ].join(' ');
}

function PieChart({
  data,
  radius = DEFAULT_RADIUS,
  emptyLabel = 'No data yet.',
  testID = 'PieChart',
}: PieChartProps) {
  const theme = useTheme();
  const total = data.reduce((sum, d) => sum + d.value, 0);

  if (data.length === 0 || total === 0) {
    return (
      <View
        height={radius * 2}
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

  const innerRadius = radius * 0.55;
  const textColor = tokenValue(theme, 'primary-foreground');
  const centerColor = tokenValue(theme, 'card');

  const slices = data.reduce<
    { datum: PieChartDatum; startAngle: number; endAngle: number }[]
  >((acc, datum) => {
    const startAngle =
      acc.length === 0 ? -Math.PI / 2 : acc[acc.length - 1].endAngle;
    const endAngle = startAngle + (datum.value / total) * Math.PI * 2;
    return [...acc, { datum, startAngle, endAngle }];
  }, []);

  return (
    <View testID={testID}>
      <XStack items="center" justify="center" gap="$6" py="$2" rounded="$xl">
        <Svg width={radius * 2} height={radius * 2}>
          <G transform={`translate(${radius} ${radius})`}>
            <Circle cx={0} cy={0} r={innerRadius} fill={centerColor} />
            {slices.map(({ datum, startAngle, endAngle }, index) => (
              <Path
                key={`slice-${index}`}
                d={slicePath(radius, innerRadius, startAngle, endAngle)}
                fill={tokenValue(theme, resolveTokenName(datum.color))}
                onPress={datum.onPress}
              />
            ))}
            {slices.map(({ datum, startAngle, endAngle }, index) => {
              if (datum.value <= 0) return null;
              const midAngle = (startAngle + endAngle) / 2;
              const midRadius = (radius + innerRadius) / 2;
              const point = polar(midRadius, midAngle);
              return (
                <SvgText
                  key={`slice-text-${index}`}
                  x={point.x}
                  y={point.y}
                  fontSize={13}
                  fill={textColor}
                  textAnchor="middle"
                  alignmentBaseline="central"
                >
                  {String(datum.value)}
                </SvgText>
              );
            })}
          </G>
        </Svg>
        <YStack gap="$2" testID={`${testID}-Legend`}>
          {data.map((d, index) => {
            const row = (
              <XStack
                items="center"
                gap="$2"
                testID={`${testID}-Legend-Item-${index}-Row`}
              >
                <YStack
                  width={12}
                  height={12}
                  rounded="$pill"
                  style={{
                    backgroundColor: tokenValue(
                      theme,
                      resolveTokenName(d.color)
                    ),
                  }}
                  testID={`${testID}-Legend-Item-${index}-Swatch`}
                />
                <Text
                  variant="small"
                  testID={`${testID}-Legend-Item-${index}-Label`}
                >
                  {d.label} ({d.value})
                </Text>
              </XStack>
            );
            // The legend row is the reliable tap target on every platform;
            // slice hit-testing on svg is left as a bonus, not the contract.
            return d.onPress ? (
              <Pressable
                key={d.label}
                onPress={d.onPress}
                testID={`${testID}-Legend-Item-${index}`}
              >
                {row}
              </Pressable>
            ) : (
              <YStack key={d.label} testID={`${testID}-Legend-Item-${index}`}>
                {row}
              </YStack>
            );
          })}
        </YStack>
      </XStack>
    </View>
  );
}

export { PieChart };
