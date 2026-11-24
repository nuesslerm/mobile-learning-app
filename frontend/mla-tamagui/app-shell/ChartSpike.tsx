import { Text, View } from '@lib/tamagui-design-system';
import {
  DashPathEffect,
  LinearGradient,
  vec,
} from '@shopify/react-native-skia';
import { useCallback, useMemo, useState, type ReactNode } from 'react';
import { Pressable, ScrollView, View as RNView } from 'react-native';
import { runOnJS, useAnimatedReaction } from 'react-native-reanimated';
import {
  Area,
  CartesianChart,
  Line,
  StackedBar,
  useChartPressState,
} from 'victory-native';

/**
 * The interaction spike for the two dashboards: tokens per day split across
 * concurrent plans, host CPU over time against the headroom ceiling, and the
 * per-container memory split.
 *
 * The data is still hardcoded, but the interactions are the real ones. The
 * page scrolls; the range control reshapes every chart; the legend toggles a
 * plan out of the stack; pressing a bar selects that day; pressing a plan in
 * the breakdown descends to its runs, which is the drill-down the agents
 * dashboard is built around (day -> plan -> run -> the run's transcript).
 *
 * Press position comes from useChartPressState and is mirrored into React
 * state, so everything outside the canvas is ordinary Tamagui: it themes
 * itself, it wraps, and a flow test can assert on it without going through
 * Skia.
 */

/** Series colours. The preset palette gives primary and accent the same hue,
 * so a categorical scale has to be its own thing -- three plans in one stack
 * are unreadable otherwise. This is what moves into the design system. */
const SERIES = [
  { key: 'dashboards', label: 'agent-dashboards', color: '#3B82F6' },
  { key: 'settings', label: 'settings-screen', color: '#A855F7' },
  { key: 'harness', label: 'flow-test-harness', color: '#14B8A6' },
] as const;

type SeriesKey = (typeof SERIES)[number]['key'];
type DayRow = { day: string } & Record<SeriesKey, number>;

const ALL_KEYS: SeriesKey[] = ['dashboards', 'settings', 'harness'];

const RANGES = [
  { id: '7d', label: '7 days', days: 7 },
  { id: '14d', label: '14 days', days: 14 },
  { id: '30d', label: '30 days', days: 30 },
] as const;

type RangeId = (typeof RANGES)[number]['id'];

/** Deterministic stand-in for the reporting service, so the range control has
 * something to reshape without a network call. */
function tokensByDay(days: number): DayRow[] {
  const labels = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
  const wave = (i: number, n: number, phase: number) =>
    Math.round(180 + 260 * Math.abs(Math.sin((i + phase) / n)));
  return Array.from({ length: days }, (_, i) => ({
    day: days > 7 ? String(i + 1) : (labels[i % 7] ?? String(i + 1)),
    dashboards: wave(i, 2.3, 0),
    settings: Math.round(wave(i, 3.1, 1.7) * 0.55),
    harness: Math.round(wave(i, 1.9, 0.4) * 0.35),
  }));
}

type Run = {
  id: string;
  unit: string;
  model: string;
  tokens: number;
  seconds: number;
};

const RUNS_BY_PLAN: Record<SeriesKey, Run[]> = {
  dashboards: [
    { id: 'a1613c30', unit: 'c1', model: 'deepseek-v4-flash', tokens: 3951, seconds: 582 },
    { id: '4f483984', unit: 'p2', model: 'deepseek-v4-flash', tokens: 793, seconds: 180 },
    { id: 'd78d1c57', unit: 'p1', model: 'deepseek-v4-flash', tokens: 2140, seconds: 341 },
  ],
  settings: [
    { id: '2f29dcfe', unit: 'n5', model: 'kimi-for-coding', tokens: 1320, seconds: 244 },
    { id: '7c55782a', unit: 'n4', model: 'kimi-for-coding', tokens: 640, seconds: 121 },
  ],
  harness: [
    { id: '14389b66', unit: 'u9', model: 'MiniMax-M3', tokens: 880, seconds: 163 },
  ],
};

/** Always-Free A1: 2 OCPU, 12 GB. The ceilings every reading is judged against. */
const CPU_CEILING = 2;
const MEMORY_CEILING_MB = 12 * 1024;

function cpuSeries(days: number): { t: number; cpu: number }[] {
  const samples = Math.min(96, days * 8);
  return Array.from({ length: samples }, (_, i) => ({
    t: i,
    cpu:
      0.55 +
      0.62 * Math.abs(Math.sin(i / 5.5)) +
      0.35 * Math.abs(Math.sin(i / 2.1)),
  }));
}

const CONTAINERS = [
  { name: 'vm-ops-api', mb: 412, color: '#3B82F6' },
  { name: 'agent-shell', mb: 1980, color: '#A855F7' },
  { name: 'opencode-log', mb: 305, color: '#14B8A6' },
  { name: 'tailscale', mb: 88, color: '#F59E0B' },
  { name: 'caddy', mb: 54, color: '#EC4899' },
];

function formatTokens(value: number): string {
  return value >= 1000 ? `${(value / 1000).toFixed(1)}k` : String(value);
}

function formatMb(mb: number): string {
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb} MB`;
}

function Dot({ color, size = 8 }: { color: string; size?: number }) {
  return (
    <RNView
      style={{
        backgroundColor: color,
        borderRadius: 999,
        height: size,
        width: size,
      }}
    />
  );
}

function Card({ children, testID }: { children: ReactNode; testID: string }) {
  return (
    <View
      bg="$card"
      borderColor="$borderColor"
      borderWidth={1}
      gap="$3"
      p="$4"
      rounded="$6"
      testID={testID}
    >
      {children}
    </View>
  );
}

function CardHeader({
  title,
  subtitle,
  value,
  valueNote,
  valueColor,
}: {
  title: string;
  subtitle: string;
  value: string;
  valueNote: string;
  valueColor?: '$color11' | '$destructive';
}) {
  return (
    <View flexDirection="row" items="flex-start" justify="space-between">
      <View flex={1} gap="$1">
        <Text variant="h3">{title}</Text>
        <Text color="$color11" variant="small">
          {subtitle}
        </Text>
      </View>
      <View items="flex-end">
        <Text variant="h3">{value}</Text>
        <Text color={valueColor ?? '$color11'} variant="small">
          {valueNote}
        </Text>
      </View>
    </View>
  );
}

function RangeControl({
  value,
  onChange,
}: {
  value: RangeId;
  onChange: (id: RangeId) => void;
}) {
  return (
    <View
      bg="$color3"
      flexDirection="row"
      gap="$1"
      p="$1"
      rounded="$10"
      testID="ChartSpikeRange"
    >
      {RANGES.map((r) => {
        const active = r.id === value;
        return (
          <Pressable
            key={r.id}
            onPress={() => onChange(r.id)}
            style={{ flex: 1 }}
            testID={`ChartSpikeRange-${r.id}`}
          >
            <View
              bg={active ? '$background' : 'transparent'}
              items="center"
              py="$2"
              rounded="$10"
            >
              <Text
                color={active ? '$color' : '$color11'}
                fontWeight={active ? '700' : '500'}
                variant="small"
              >
                {r.label}
              </Text>
            </View>
          </Pressable>
        );
      })}
    </View>
  );
}

function TokensCard({ days }: { days: number }) {
  const [hidden, setHidden] = useState<SeriesKey[]>([]);
  const [selected, setSelected] = useState(3);
  const [drilled, setDrilled] = useState<SeriesKey | null>(null);
  const { state } = useChartPressState({
    x: '',
    y: { dashboards: 0, settings: 0, harness: 0 },
  });

  const rows = useMemo(() => tokensByDay(days), [days]);
  const maxStack = useMemo(
    () => Math.max(...rows.map((d) => ALL_KEYS.reduce((s, k) => s + d[k], 0))),
    [rows]
  );

  // yKeys has to stay the same tuple for CartesianChart's inference, so a
  // hidden series is zeroed rather than removed -- which also lets the stack
  // animate down instead of the chart re-keying under it.
  const data = useMemo(
    () =>
      rows.map((d) => ({
        ...d,
        ...Object.fromEntries(hidden.map((k) => [k, 0])),
      })) as DayRow[],
    [rows, hidden]
  );

  const select = useCallback(
    (index: number) => {
      setSelected(Math.min(index, rows.length - 1));
      setDrilled(null);
    },
    [rows.length]
  );

  useAnimatedReaction(
    () => (state.isActive.value ? state.matchedIndex.value : -1),
    (index, previous) => {
      // -1 is "not pressing", not "pressed nothing": reacting to it cleared
      // the selection the moment the finger lifted.
      if (index >= 0 && index !== previous) runOnJS(select)(index);
    }
  );

  const toggle = useCallback((key: SeriesKey) => {
    setHidden((current) =>
      current.includes(key)
        ? current.filter((k) => k !== key)
        : current.length + 1 < SERIES.length
          ? [...current, key]
          : current
    );
  }, []);

  const visible = SERIES.filter((s) => !hidden.includes(s.key));
  const index = Math.min(selected, data.length - 1);
  const row = data[index] as DayRow;
  const rowTotal = visible.reduce((sum, s) => sum + row[s.key], 0);
  const total = data.reduce(
    (sum, d) => sum + visible.reduce((s, k) => s + d[k.key], 0),
    0
  );

  return (
    <Card testID="ChartSpikeTokens">
      <CardHeader
        subtitle="by day, split across concurrent plans"
        title="Token spend"
        value={formatTokens(total)}
        valueNote={`${data.length} days`}
      />

      <View flexDirection="row" flexWrap="wrap" gap="$2">
        {SERIES.map((s) => {
          const active = !hidden.includes(s.key);
          return (
            <Pressable
              key={s.key}
              onPress={() => toggle(s.key)}
              testID={`ChartSpikeLegend-${s.key}`}
            >
              <View
                bg={active ? '$color3' : 'transparent'}
                borderColor="$borderColor"
                borderWidth={1}
                flexDirection="row"
                gap="$2"
                items="center"
                px="$3"
                py="$1.5"
                rounded="$10"
              >
                <RNView style={{ opacity: active ? 1 : 0.3 }}>
                  <Dot color={s.color} size={10} />
                </RNView>
                <Text color={active ? '$color' : '$color11'} variant="small">
                  {s.label}
                </Text>
              </View>
            </Pressable>
          );
        })}
      </View>

      <View height={210} testID="ChartSpikeTokensCanvas">
        <CartesianChart
          chartPressState={state}
          data={data}
          domain={{ y: [0, maxStack * 1.12] }}
          domainPadding={{ left: 16, right: 16, top: 6 }}
          xKey="day"
          yKeys={ALL_KEYS}
        >
          {({ points, chartBounds }) => (
            <StackedBar
              animate={{ type: 'spring' }}
              barOptions={({ isBottom, isTop, datumIndex }) => ({
                roundedCorners: {
                  topLeft: isTop ? 5 : 0,
                  topRight: isTop ? 5 : 0,
                  bottomLeft: isBottom ? 2 : 0,
                  bottomRight: isBottom ? 2 : 0,
                },
                opacity: datumIndex === index ? 1 : 0.4,
              })}
              barWidth={data.length > 14 ? 7 : data.length > 7 ? 14 : 24}
              chartBounds={chartBounds}
              colors={SERIES.map((s) => s.color)}
              points={SERIES.map((s) => points[s.key])}
            />
          )}
        </CartesianChart>
      </View>

      {data.length <= 14 ? (
        <View flexDirection="row" justify="space-between" px="$2">
          {data.map((d, i) => (
            <Text
              color={i === index ? '$color' : '$color11'}
              fontWeight={i === index ? '700' : '400'}
              key={`${d.day}-${i}`}
              variant="small"
            >
              {d.day}
            </Text>
          ))}
        </View>
      ) : null}

      <View bg="$color3" gap="$2" p="$3" rounded="$4" testID="ChartSpikeBreakdown">
        {drilled === null ? (
          <>
            <View flexDirection="row" justify="space-between">
              <Text fontWeight="700">{row.day}</Text>
              <Text color="$color11" variant="small">
                {formatTokens(rowTotal)} tokens · tap a plan
              </Text>
            </View>
            {visible.map((s) => {
              const share = rowTotal === 0 ? 0 : row[s.key] / rowTotal;
              const runs = RUNS_BY_PLAN[s.key];
              return (
                <Pressable
                  key={s.key}
                  onPress={() => setDrilled(s.key)}
                  testID={`ChartSpikePlan-${s.key}`}
                >
                  <View gap="$1.5" py="$1">
                    <View flexDirection="row" gap="$2" items="center">
                      <Dot color={s.color} />
                      <Text flex={1} variant="small">
                        {s.label}
                      </Text>
                      <Text color="$color11" variant="small">
                        {formatTokens(row[s.key])} · {runs.length} run
                        {runs.length === 1 ? '' : 's'} ›
                      </Text>
                    </View>
                    <View bg="$borderColor" height={4} rounded={999} width="100%">
                      <RNView
                        style={{
                          backgroundColor: s.color,
                          borderRadius: 999,
                          height: 4,
                          width: `${Math.round(share * 100)}%`,
                        }}
                      />
                    </View>
                  </View>
                </Pressable>
              );
            })}
          </>
        ) : (
          <>
            <Pressable
              onPress={() => setDrilled(null)}
              testID="ChartSpikeDrillBack"
            >
              <View flexDirection="row" gap="$2" items="center" py="$1">
                <Text color="$color11" variant="small">
                  ‹
                </Text>
                <Dot
                  color={SERIES.find((s) => s.key === drilled)?.color ?? '#888'}
                />
                <Text flex={1} fontWeight="700" variant="small">
                  {SERIES.find((s) => s.key === drilled)?.label}
                </Text>
                <Text color="$color11" variant="small">
                  {row.day}
                </Text>
              </View>
            </Pressable>
            {RUNS_BY_PLAN[drilled].map((run) => (
              <Pressable key={run.id} testID={`ChartSpikeRun-${run.id}`}>
                <View
                  bg="$background"
                  flexDirection="row"
                  gap="$2"
                  items="center"
                  p="$2.5"
                  rounded="$3"
                >
                  <View flex={1} gap="$1">
                    <Text fontWeight="600" variant="small">
                      {run.unit} · {run.model}
                    </Text>
                    <Text color="$color11" variant="small">
                      {run.id} · {formatTokens(run.tokens)} tokens · {run.seconds}s
                    </Text>
                  </View>
                  <Text color="$color11">›</Text>
                </View>
              </Pressable>
            ))}
          </>
        )}
      </View>
    </Card>
  );
}

function CpuCard({ days }: { days: number }) {
  const [reading, setReading] = useState<number | null>(null);
  const { state } = useChartPressState({ x: 0, y: { cpu: 0 } });
  const series = useMemo(() => cpuSeries(days), [days]);

  const setIndex = useCallback((next: number) => {
    setReading(next);
  }, []);

  useAnimatedReaction(
    () => (state.isActive.value ? state.matchedIndex.value : -1),
    (next, previous) => {
      if (next >= 0 && next !== previous) runOnJS(setIndex)(next);
    }
  );

  const peak = Math.max(...series.map((d) => d.cpu));
  const shown =
    reading === null || reading >= series.length
      ? (series[series.length - 1] as { t: number; cpu: number })
      : (series[reading] as { t: number; cpu: number });
  const headroom = ((CPU_CEILING - shown.cpu) / CPU_CEILING) * 100;

  return (
    <Card testID="ChartSpikeCpu">
      <CardHeader
        subtitle={
          reading === null
            ? `${series.length} samples · drag to scrub`
            : `sample ${reading + 1} of ${series.length}`
        }
        title="Host CPU"
        value={`${shown.cpu.toFixed(2)} OCPU`}
        valueColor={headroom < 20 ? '$destructive' : '$color11'}
        valueNote={`${headroom.toFixed(0)}% headroom`}
      />

      <View height={190} testID="ChartSpikeCpuCanvas">
        <CartesianChart
          chartPressState={state}
          data={series}
          domain={{ y: [0, CPU_CEILING * 1.12] }}
          xKey="t"
          yKeys={['cpu']}
        >
          {({ points, chartBounds }) => {
            const ceilingY =
              chartBounds.bottom -
              ((chartBounds.bottom - chartBounds.top) * CPU_CEILING) /
                (CPU_CEILING * 1.12);
            return (
              <>
                <Area
                  animate={{ type: 'timing' }}
                  curveType="monotoneX"
                  points={points.cpu}
                  y0={chartBounds.bottom}
                >
                  <LinearGradient
                    colors={['#3B82F6AA', '#3B82F600']}
                    end={vec(chartBounds.left, chartBounds.bottom)}
                    start={vec(chartBounds.left, chartBounds.top)}
                  />
                </Area>
                <Line
                  animate={{ type: 'timing' }}
                  color="#3B82F6"
                  curveType="monotoneX"
                  points={points.cpu}
                  strokeWidth={2.5}
                />
                <Line
                  color="#EF4444"
                  points={points.cpu.map((p) => ({ ...p, y: ceilingY }))}
                  strokeWidth={1.5}
                >
                  <DashPathEffect intervals={[6, 5]} />
                </Line>
              </>
            );
          }}
        </CartesianChart>
      </View>

      <View
        bg="$color3"
        flexDirection="row"
        justify="space-between"
        p="$3"
        rounded="$4"
        testID="ChartSpikeCpuFooter"
      >
        <Text color="$color11" variant="small">
          Ceiling {CPU_CEILING.toFixed(1)} OCPU (Always-Free A1)
        </Text>
        <Text color="$color11" variant="small">
          Peak {peak.toFixed(2)}
        </Text>
      </View>
    </Card>
  );
}

function ContainersCard() {
  const [open, setOpen] = useState<string | null>(null);
  const used = CONTAINERS.reduce((sum, c) => sum + c.mb, 0);
  const free = MEMORY_CEILING_MB - used;

  return (
    <Card testID="ChartSpikeContainers">
      <CardHeader
        subtitle="per container, sampled every 60s"
        title="Memory"
        value={formatMb(used)}
        valueNote={`${formatMb(free)} free of ${formatMb(MEMORY_CEILING_MB)}`}
      />

      <View
        flexDirection="row"
        height={18}
        overflow="hidden"
        rounded="$3"
        testID="ChartSpikeMemoryBar"
        width="100%"
      >
        {CONTAINERS.map((c) => (
          <RNView
            key={c.name}
            style={{
              backgroundColor: c.color,
              flexGrow: c.mb,
              opacity: open === null || open === c.name ? 1 : 0.35,
            }}
          />
        ))}
        <RNView style={{ backgroundColor: '#0000001A', flexGrow: free }} />
      </View>

      <View gap="$1">
        {CONTAINERS.map((c) => (
          <Pressable
            key={c.name}
            onPress={() => setOpen(open === c.name ? null : c.name)}
            testID={`ChartSpikeContainer-${c.name}`}
          >
            <View
              bg={open === c.name ? '$color3' : 'transparent'}
              flexDirection="row"
              gap="$2"
              items="center"
              p="$2"
              rounded="$3"
            >
              <Dot color={c.color} />
              <Text flex={1} variant="small">
                {c.name}
              </Text>
              <Text color="$color11" variant="small">
                {formatMb(c.mb)} ·{' '}
                {((c.mb / MEMORY_CEILING_MB) * 100).toFixed(1)}%
              </Text>
            </View>
          </Pressable>
        ))}
      </View>
    </Card>
  );
}

export default function ChartSpike() {
  const [range, setRange] = useState<RangeId>('7d');
  const days = RANGES.find((r) => r.id === range)?.days ?? 7;

  return (
    <ScrollView
      contentContainerStyle={{ paddingBottom: 64 }}
      testID="ChartSpikeScroll"
    >
      <View bg="$background" gap="$4" p="$4" pt="$6">
        <View gap="$1">
          <Text testID="ChartSpikeHeading" variant="h1">
            Chart spike
          </Text>
          <Text color="$color11" testID="ChartSpikeSubtitle" variant="muted">
            victory-native on Skia: the interactions both dashboards need
          </Text>
        </View>
        <RangeControl onChange={setRange} value={range} />
        <TokensCard days={days} />
        <CpuCard days={days} />
        <ContainersCard />
      </View>
    </ScrollView>
  );
}
