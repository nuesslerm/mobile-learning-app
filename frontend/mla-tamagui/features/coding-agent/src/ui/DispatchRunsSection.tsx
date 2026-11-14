import { router } from 'expo-router';
import type { DispatchRunMeta } from '../domain';
import {
  Badge,
  InsetGroup,
  ListItem,
  Text,
  View,
} from '@lib/tamagui-design-system';

import { useDispatchRuns } from '../state/useDispatchRuns';
import type { CodingAgentTransport } from '../state/gatewayStore';

export type DispatchRunsSectionProps = {
  transport: CodingAgentTransport;
};

const STATUS_DOT_COLOR = {
  running: '$accent9',
  finishedOk: '$green9',
  finishedFailed: '$red9',
} as const;

const PROMPT_EXCERPT_MAX = 80;

function promptExcerpt(prompt: string): string {
  return prompt.replace(/\s+/g, ' ').trim().slice(0, PROMPT_EXCERPT_MAX);
}

function formatRelativeTime(iso: string): string {
  const elapsedMs = Date.now() - Date.parse(iso);
  if (Number.isNaN(elapsedMs)) return '';
  const seconds = Math.floor(elapsedMs / 1000);
  if (seconds < 60) return 'just now';
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

function statusColor(
  run: DispatchRunMeta,
): (typeof STATUS_DOT_COLOR)[keyof typeof STATUS_DOT_COLOR] {
  if (run.status === 'running') return STATUS_DOT_COLOR.running;
  if (run.ok === true) return STATUS_DOT_COLOR.finishedOk;
  return STATUS_DOT_COLOR.finishedFailed;
}

function DispatchRunRow({
  run,
  transport,
}: {
  run: DispatchRunMeta;
  transport: CodingAgentTransport;
}): React.ReactElement {
  const costLabel =
    run.status === 'finished' && run.cost !== null ? `$${run.cost.toFixed(4)}` : null;
  const href =
    transport.length > 0
      ? `/coding-agent/runs/${run.runId}?transport=${transport}`
      : `/coding-agent/runs/${run.runId}`;
  return (
    <ListItem
      variant="grouped"
      testID={`DispatchRunRow-${run.runId}`}
      showChevron
      onPress={() => {
        router.push(href);
      }}
    >
      <View flexDirection="row" items="center" gap="$1.5" flexWrap="wrap">
        {run.agent !== null && run.agent !== undefined && run.agent.length > 0 ? (
          <Badge variant="secondary" testID={`DispatchRunRowAgent-${run.runId}`}>
            {run.agent}
          </Badge>
        ) : null}
        {run.model !== null && run.model !== undefined && run.model.length > 0 ? (
          <Badge variant="outline" testID={`DispatchRunRowModel-${run.runId}`}>
            {run.model}
          </Badge>
        ) : null}
      </View>
      <Text variant="muted" size="$3" numberOfLines={1}>
        {promptExcerpt(run.prompt)}
      </Text>
      <View flexDirection="row" items="center" gap="$1.5" justify="space-between">
        <View flexDirection="row" items="center" gap="$1.5">
          <View
            width={8}
            height={8}
            rounded={9999}
            bg={statusColor(run)}
            testID={`DispatchRunRowStatus-${run.runId}`}
          />
          <Text variant="muted" size="$3">
            {formatRelativeTime(run.startedAt)}
          </Text>
        </View>
        {costLabel !== null ? (
          <Text variant="muted" size="$3" testID={`DispatchRunRowCost-${run.runId}`}>
            {costLabel}
          </Text>
        ) : null}
      </View>
    </ListItem>
  );
}

export function DispatchRunsSection({
  transport,
}: DispatchRunsSectionProps): React.ReactElement | null {
  const { runs, enabled } = useDispatchRuns(transport);
  if (!enabled || runs.length === 0) {
    return null;
  }
  return (
    <View gap="$2">
      <Text variant="h3" px="$4" testID="DispatchRunsSectionTitle">
        Dispatch runs
      </Text>
      <InsetGroup testID="DispatchRunsSection">
        {runs.map((run) => (
          <DispatchRunRow key={run.runId} run={run} transport={transport} />
        ))}
      </InsetGroup>
    </View>
  );
}
