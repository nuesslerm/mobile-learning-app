import { useCallback, useMemo } from 'react';
import { Linking } from 'react-native';
import { router } from 'expo-router';
import { useDependencyOverrides } from '@lib/app-di';
import { appConfig } from '@lib/app-config';
import { buildDispatchRunFeed } from '../domain';
import { Logger } from '@lib/logging';
import {
  Button,
  ScreenHeader,
  Spinner,
  Text,
  View,
} from '@lib/tamagui-design-system';

import type { OrchestrationGateway } from '../data/orchestrationGateway';
import { dispatchRunDashboardUrl } from '../data/dispatchRunsGateway';
import { useDispatchRun } from '../integration/useDispatchRuns';
import type { CodingAgentTransport } from '../state/gatewayStore';
import { ThreadFeed } from './feed/ThreadFeed';

const COST_FORMAT = new Intl.NumberFormat('en-US', {
  minimumFractionDigits: 4,
  maximumFractionDigits: 4,
});

const FIXTURE_DASHBOARD_BASE_URL = 'http://vm-ops-api-main:8080';

function formatSubtitle(
  detail:
    | {
        status: 'running' | 'finished';
        tokens: { total: number; input: number; output: number } | null;
        cost: number | null;
        durationMs: number | null;
      }
    | undefined
): string {
  if (detail === undefined || detail.status !== 'finished') {
    return 'running';
  }
  const seconds = detail.durationMs !== null ? Math.round(detail.durationMs / 1000) : 0;
  const tokens = detail.tokens?.total ?? 0;
  const cost = detail.cost !== null ? `$${COST_FORMAT.format(detail.cost)}` : '$0.0000';
  return `${tokens} tokens · ${cost} · ${seconds}s`;
}

export type DispatchRunScreenProps = {
  runId: string;
};

export function DispatchRunScreen({
  runId,
}: DispatchRunScreenProps): React.ReactElement {
  const { gateway: overriddenGateway } =
    useDependencyOverrides<{ gateway: OrchestrationGateway }>('coding-agent');
  const dispatchTransport: CodingAgentTransport =
    overriddenGateway !== undefined ? 'fixture' : 'ws';
  const { detail, status, error, dashboardUrl } = useDispatchRun(
    dispatchTransport,
    runId,
  );

  const effectiveDashboardUrl = useMemo(() => {
    if (dashboardUrl !== undefined) return dashboardUrl;
    if (dispatchTransport !== 'fixture') return undefined;
    const baseUrl = appConfig.vmOpsApiBaseUrl ?? FIXTURE_DASHBOARD_BASE_URL;
    return dispatchRunDashboardUrl(baseUrl, runId);
  }, [dashboardUrl, dispatchTransport, runId]);

  const feedView = useMemo(() => {
    if (detail === undefined) {
      return { feed: [], reasoningByTurnId: new Map<string, string>() };
    }
    const built = buildDispatchRunFeed(detail);
    return {
      feed: built.feed,
      reasoningByTurnId: new Map<string, string>(
        Object.entries(built.reasoningByTurnId),
      ),
      streamingTurnId: built.streamingTurnId,
    };
  }, [detail]);

  const handleSetLiveFollowEvent = useCallback(() => {}, []);

  const handleOpenDashboard = useCallback(() => {
    if (effectiveDashboardUrl === undefined) return;
    Logger.withMetadata({ runId, dashboardUrl: effectiveDashboardUrl }).info(
      'coding-agent.dispatch_run.open_dashboard',
    );
    Linking.openURL(effectiveDashboardUrl).catch((cause: unknown) => {
      Logger.withError(cause).warn(
        'coding-agent.dispatch_run.open_dashboard_failed',
        { runId },
      );
    });
  }, [effectiveDashboardUrl, runId]);

  const title = detail?.model ?? 'Dispatch run';
  const subtitle = formatSubtitle(detail);

  return (
    <View flex={1} testID="DispatchRunScreen">
      <ScreenHeader
        title={title}
        subtitle={subtitle}
        onBack={() => {
          router.back();
        }}
        testID="DispatchRunHeader"
      />
      {status === 'loading' && detail === undefined ? (
        <View flex={1} items="center" justify="center" testID="DispatchRunLoading">
          <Spinner />
        </View>
      ) : status === 'error' && detail === undefined ? (
        <View flex={1} items="center" justify="center" p="$6">
          <Text testID="DispatchRunError">{error ?? 'Unknown error'}</Text>
        </View>
      ) : (
        <>
          <View flex={1} testID="DispatchRunFeed">
            <ThreadFeed
              feed={feedView.feed}
              reasoningByTurnId={feedView.reasoningByTurnId}
              streamingTurnId={feedView.streamingTurnId}
              liveFollow
              setLiveFollowEvent={handleSetLiveFollowEvent}
            />
          </View>
          {effectiveDashboardUrl !== undefined ? (
            <View p="$4">
              <Button
                variant="secondary"
                onPress={handleOpenDashboard}
                testID="DispatchRunDashboardLink"
              >
                Open in dashboard
              </Button>
            </View>
          ) : null}
        </>
      )}
    </View>
  );
}
