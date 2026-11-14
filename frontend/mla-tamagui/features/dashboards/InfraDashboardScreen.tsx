import { Logger } from '@lib/logging';
import {
  Card,
  CardContent,
  CardHeader,
  ScreenHeader,
  SegmentedControl,
  Spinner,
  Text,
  TimeSeriesChart,
  View,
  type TimeSeries,
} from '@lib/tamagui-design-system';
import { appConfig } from '@lib/app-config';
import { useDependencyOverrides } from '@lib/app-di';
import { backTo } from '@feature/settings';
import { useEffect, useMemo, useState } from 'react';
import { ScrollView } from 'react-native';
import { YStack } from '@lib/tamagui-design-system/primitives';

import {
  createHttpReportingGateway,
  type ReportingGateway,
  type ReportingResult,
  type SeriesWindow,
} from './data/reportingGateway';
import type {
  HeadroomResourceReport,
  MetricSample,
  MetricsCurrentResponse,
  MetricsSeriesResponse,
  HeadroomResponse,
} from './data/reportingSchema';

const GiB = 1024 ** 3;
const OCPU_CEILING = 2;
const MEMORY_CEILING_GB = 12;

const WINDOW_OPTIONS: { value: SeriesWindow; label: string }[] = [
  { value: '1h', label: '1h' },
  { value: '24h', label: '24h' },
  { value: '7d', label: '7d' },
  { value: '30d', label: '30d' },
];

type InfraState =
  | { status: 'loading' }
  | { status: 'not-configured' }
  | { status: 'unreachable'; reason: string }
  | { status: 'ready'; data: InfraData };

interface ContainerView {
  name: string;
  cpu: TimeSeries;
  memory: TimeSeries;
  storageText: string;
}

interface InfraData {
  hostCpu: TimeSeries;
  hostMemory: TimeSeries;
  containers: ContainerView[];
  headroomSentence: string;
}

function formatOcpu(value: number): string {
  return `${value.toFixed(1)} OCPU`;
}

function formatBytes(bytes: number): string {
  if (bytes >= GiB) return `${(bytes / GiB).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${bytes} B`;
}

function buildSeries(
  points: MetricSample[],
  scope: string,
  name: string,
  metric: string,
  key: string,
  transform: (value: number) => number,
): TimeSeries {
  return {
    key,
    points: points
      .filter(
        (p) => p.scope === scope && p.name === name && p.metric === metric,
      )
      .map((p) => ({ t: p.ts, v: transform(p.value) })),
  };
}

function latestByKey(samples: MetricSample[]): Map<string, MetricSample> {
  const latest = new Map<string, MetricSample>();
  for (const sample of samples) {
    latest.set(`${sample.scope}\u0000${sample.name}\u0000${sample.metric}`, sample);
  }
  return latest;
}

function containerFits(
  containerMemoryBytes: number[],
  remainingMemoryBytes: number,
): boolean {
  if (containerMemoryBytes.length === 0) return remainingMemoryBytes > 0;
  return Math.max(...containerMemoryBytes) <= remainingMemoryBytes;
}

function buildInfraData(
  seriesPoints: MetricSample[],
  currentSamples: MetricSample[],
  resources: HeadroomResourceReport[],
): InfraData {
  const hostCpu = buildSeries(
    seriesPoints,
    'host',
    'host',
    'cpu',
    'CPU (OCPU)',
    (value) => (value / 100) * OCPU_CEILING,
  );
  const hostMemory = buildSeries(
    seriesPoints,
    'host',
    'host',
    'memory_used',
    'Memory (GB)',
    (value) => value / GiB,
  );

  const containerNames = [
    ...new Set(
      seriesPoints
        .filter(
          (p) =>
            p.scope === 'container' &&
            (p.metric === 'cpu' || p.metric === 'memory'),
        )
        .map((p) => p.name),
    ),
  ].sort();

  const latest = latestByKey(currentSamples);
  const containers: ContainerView[] = containerNames.map((name) => {
    const cpu = buildSeries(
      seriesPoints,
      'container',
      name,
      'cpu',
      'CPU',
      (value) => value,
    );
    const memory = buildSeries(
      seriesPoints,
      'container',
      name,
      'memory',
      'Memory (GB)',
      (value) => value / GiB,
    );
    const rootfs = latest.get(`container\u0000${name}\u0000size_rootfs`);
    const rw = latest.get(`container\u0000${name}\u0000size_rw`);
    const storageValue = rootfs?.value ?? rw?.value ?? null;
    const storageText =
      storageValue == null ? 'No storage figure' : formatBytes(storageValue);
    return { name, cpu, memory, storageText };
  });

  const cpuRes = resources.find((r) => r.resource === 'cpu');
  const memRes = resources.find((r) => r.resource === 'memory');
  const cpuRemaining = cpuRes?.remaining ?? 0;
  const memRemaining = memRes?.remaining ?? 0;

  const containerMemory = [...latest.values()]
    .filter((s) => s.scope === 'container' && s.metric === 'memory')
    .map((s) => s.value);
  const fits = containerFits(containerMemory, memRemaining);

  const headroomSentence = [
    `You have ${formatOcpu(cpuRemaining)} and ${formatBytes(memRemaining)} of the Always-Free ceiling free.`,
    fits
      ? 'Another container would fit.'
      : 'Not enough headroom for another container.',
  ].join(' ');

  return { hostCpu, hostMemory, containers, headroomSentence };
}

function resolveState(
  series: ReportingResult<MetricsSeriesResponse>,
  current: ReportingResult<MetricsCurrentResponse>,
  headroom: ReportingResult<HeadroomResponse>,
): InfraState {
  if (
    series.status === 'not-configured' ||
    current.status === 'not-configured' ||
    headroom.status === 'not-configured'
  ) {
    return { status: 'not-configured' };
  }
  for (const result of [series, current, headroom]) {
    if (result.status === 'unreachable') {
      return { status: 'unreachable', reason: result.reason };
    }
  }
  if (
    series.status !== 'ready' ||
    current.status !== 'ready' ||
    headroom.status !== 'ready'
  ) {
    return { status: 'loading' };
  }
  return {
    status: 'ready',
    data: buildInfraData(
      series.data.points,
      current.data.samples,
      headroom.data.resources,
    ),
  };
}

export function InfraDashboardScreen() {
  const [window, setWindow] = useState<SeriesWindow>('24h');
  const [state, setState] = useState<InfraState>({ status: 'loading' });

  const { reportingGateway: overrideGateway } =
    useDependencyOverrides<{ reportingGateway: ReportingGateway }>(
      'dashboards',
    );
  const reportingGateway = useMemo(
    () =>
      overrideGateway ?? createHttpReportingGateway(appConfig.reportingBaseUrl),
    [overrideGateway],
  );

  useEffect(() => {
    const gateway = reportingGateway;
    let cancelled = false;
    void Promise.all([
      gateway.getMetricsSeries(window),
      gateway.getMetricsCurrent(),
      gateway.getHeadroom(),
    ]).then(([series, current, headroom]) => {
      if (cancelled) return;
      setState(resolveState(series, current, headroom));
    });
    return () => {
      cancelled = true;
    };
  }, [window]);

  return (
    <View flex={1} bg="$background" testID="InfraDashboardScreen">
      <ScreenHeader
        testID="InfraDashboardScreenHeader"
        title="Infrastructure"
        onBack={() => {
          Logger.info('dashboards.infra.back', { to: '/dashboards' });
          backTo('/dashboards');
        }}
      />
      <ScrollView
        testID="InfraDashboardScreen-ScrollView"
        contentContainerStyle={{ paddingVertical: 16, gap: 12 }}
      >
        <View mx="$4" testID="InfraWindowSelector">
          <SegmentedControl
            options={WINDOW_OPTIONS}
            value={window}
            onChange={(value) => {
              setState({ status: 'loading' });
              setWindow(value as SeriesWindow);
            }}
            pressLog="off"
            testID="InfraWindowSelectorControl"
          />
        </View>
        {state.status === 'loading' ? (
          <View
            items="center"
            py="$6"
            testID="InfraDashboardScreen-Loading"
          >
            <Spinner testID="InfraDashboardScreen-LoadingSpinner" />
          </View>
        ) : state.status === 'not-configured' ? (
          <Text
            variant="muted"
            mx="$4"
            testID="InfraDashboardScreen-NotConfigured"
          >
            Reporting service is not configured, so there is nothing to chart.
          </Text>
        ) : state.status === 'unreachable' ? (
          <Text
            variant="muted"
            mx="$4"
            testID="InfraDashboardScreen-Unreachable"
          >
            Reporting service unreachable: {state.reason}. Charts are
            unavailable until it recovers.
          </Text>
        ) : (
          <YStack gap="$3" px="$4" testID="InfraDashboardScreen-Content">
            <Text
              variant="muted"
              testID="InfraDashboardScreen-Headroom"
            >
              {state.data.headroomSentence}
            </Text>
            <Card testID="InfraHostCpuCard">
              <CardHeader>
                <Text variant="large" testID="InfraHostCpuTitle">
                  Host CPU
                </Text>
              </CardHeader>
              <CardContent>
                <TimeSeriesChart
                  series={[state.data.hostCpu]}
                  threshold={{
                    value: OCPU_CEILING,
                    label: '2 OCPU ceiling',
                  }}
                  height={160}
                  emptyLabel="No host CPU samples in this window."
                  testID="InfraHostCpuChart"
                />
              </CardContent>
            </Card>
            <Card testID="InfraHostMemoryCard">
              <CardHeader>
                <Text variant="large" testID="InfraHostMemoryTitle">
                  Host memory
                </Text>
              </CardHeader>
              <CardContent>
                <TimeSeriesChart
                  series={[state.data.hostMemory]}
                  threshold={{
                    value: MEMORY_CEILING_GB,
                    label: '12 GB ceiling',
                  }}
                  height={160}
                  emptyLabel="No host memory samples in this window."
                  testID="InfraHostMemoryChart"
                />
              </CardContent>
            </Card>
            <Text variant="large" testID="InfraContainersTitle">
              Containers
            </Text>
            {state.data.containers.length === 0 ? (
              <Text variant="muted" testID="InfraContainersEmpty">
                No containers sampled in this window.
              </Text>
            ) : (
              state.data.containers.map((container) => (
                <Card
                  key={container.name}
                  testID={`InfraContainerCard-${container.name}`}
                >
                  <CardHeader>
                    <Text
                      variant="large"
                      testID={`InfraContainerTitle-${container.name}`}
                    >
                      {container.name}
                    </Text>
                  </CardHeader>
                  <CardContent gap="$3">
                    <TimeSeriesChart
                      series={[container.cpu]}
                      height={110}
                      emptyLabel="No CPU samples in this window."
                      testID={`InfraContainerCpu-${container.name}`}
                    />
                    <TimeSeriesChart
                      series={[container.memory]}
                      height={110}
                      emptyLabel="No memory samples in this window."
                      testID={`InfraContainerMemory-${container.name}`}
                    />
                    <Text
                      variant="small"
                      testID={`InfraContainerStorage-${container.name}`}
                    >
                      Storage: {container.storageText}
                    </Text>
                  </CardContent>
                </Card>
              ))
            )}
          </YStack>
        )}
      </ScrollView>
    </View>
  );
}