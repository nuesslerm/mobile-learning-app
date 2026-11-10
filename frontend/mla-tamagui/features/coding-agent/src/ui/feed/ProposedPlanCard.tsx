import type { ThreadFeedEntry } from '../../domain';
import type { OrchestrationProposedPlan } from '@app/remote-chat-agent-contracts/orchestration';
import { Button, Card, Markdown, Text } from '@lib/tamagui-design-system';

import { Rocket } from '../icons';

export type ProposedPlanCardProps = {
  entry: Extract<ThreadFeedEntry, { kind: 'proposed-plan' }>;
  onImplement?: (plan: OrchestrationProposedPlan) => void;
};

export function ProposedPlanCard({
  entry,
  onImplement,
}: ProposedPlanCardProps): React.ReactElement {
  return (
    <Card tone="accent" testID={`PlanCard-${entry.id}`} gap="$2">
      <Text variant="strong">Proposed plan</Text>
      <Markdown content={entry.plan.planMarkdown} />
      <Button
        testID={`PlanCardImplement-${entry.id}`}
        icon={Rocket}
        onPress={() => onImplement?.(entry.plan)}
      >
        Implement
      </Button>
    </Card>
  );
}
