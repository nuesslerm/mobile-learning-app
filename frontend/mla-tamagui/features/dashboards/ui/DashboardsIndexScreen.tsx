import { Logger } from '@lib/logging';
import {
  Icon,
  InsetGroup,
  ListItem,
  ScreenHeader,
  Text,
  View,
} from '@lib/tamagui-design-system';
import { Bot, Cpu } from '@lib/tamagui-design-system/icons';
import { backTo } from '@lib/tamagui-design-system/lib/backTo';
import { router } from 'expo-router';
import type { Href } from 'expo-router';
import { useEffect, useState } from 'react';
import { ScrollView } from 'react-native';

import { useDashboardsPorts } from '../state/ports';

type DataAgeState =
  | { status: 'loading' }
  | { status: 'not-configured' }
  | { status: 'unreachable'; reason: string }
  | { status: 'ready'; ageMs: number };

function formatAge(ms: number): string {
  const seconds = Math.max(0, Math.round(ms / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  return `${Math.floor(hours / 24)}d`;
}

function dataAgeLine(state: DataAgeState): string {
  switch (state.status) {
    case 'loading':
      return 'Checking reporting data…';
    case 'not-configured':
      return 'Reporting service is not configured.';
    case 'unreachable':
      return `Reporting service unreachable: ${state.reason}`;
    case 'ready':
      return `Data sampled ${formatAge(state.ageMs)} ago`;
  }
}

function DashboardRow({
  icon,
  label,
  href,
  testID,
}: {
  icon: typeof Cpu;
  label: string;
  href: Href;
  testID: string;
}) {
  return (
    <ListItem
      variant="grouped"
      showChevron
      pressLog="off"
      onPress={() => {
        Logger.info('dashboards.index.row_pressed', { href, label });
        router.push(href);
      }}
      testID={testID}
    >
      <View
        flexDirection="row"
        items="center"
        gap="$3"
        flex={1}
        testID={`${testID}Content`}
      >
        <Icon as={icon} size={18} color="$color11" testID={`${testID}Icon`} />
        <Text flex={1} testID={`${testID}Label`}>
          {label}
        </Text>
      </View>
    </ListItem>
  );
}

export function DashboardsIndexScreen() {
  const [dataAge, setDataAge] = useState<DataAgeState>({ status: 'loading' });
  const { reportingGateway } = useDashboardsPorts();

  useEffect(() => {
    const gateway = reportingGateway;
    let cancelled = false;
    void gateway.getMetricsCurrent().then((result) => {
      if (cancelled) return;
      if (result.status === 'ready') {
        const latestTs = result.data.samples.reduce(
          (max, sample) => Math.max(max, sample.ts),
          0
        );
        setDataAge({
          status: 'ready',
          ageMs: latestTs > 0 ? Date.now() - latestTs : 0,
        });
      } else if (result.status === 'not-configured') {
        setDataAge({ status: 'not-configured' });
      } else {
        setDataAge({ status: 'unreachable', reason: result.reason });
      }
    });
    return () => {
      cancelled = true;
    };
  }, [reportingGateway]);

  return (
    <View flex={1} bg="$background" testID="DashboardsIndexScreen">
      <ScreenHeader
        testID="DashboardsIndexScreenHeader"
        title="Dashboards"
        onBack={() => {
          Logger.info('dashboards.index.back', { to: '/' });
          backTo('/');
        }}
      />
      <ScrollView
        testID="DashboardsIndexScreen-ScrollView"
        contentContainerStyle={{ paddingVertical: 16, gap: 12 }}
      >
        <InsetGroup testID="DashboardsIndexScreen-Group">
          <DashboardRow
            icon={Cpu}
            label="Infrastructure"
            href={'/dashboards/infra' as Href}
            testID="DashboardsRow-Infrastructure"
          />
          <DashboardRow
            icon={Bot}
            label="Agents"
            href={'/dashboards/agents' as Href}
            testID="DashboardsRow-Agents"
          />
        </InsetGroup>
        <Text
          variant="muted"
          mx="$4"
          testID="DashboardsIndexScreen-DataAgeLine"
        >
          {dataAgeLine(dataAge)}
        </Text>
      </ScrollView>
    </View>
  );
}