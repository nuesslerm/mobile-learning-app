import {
  type PendingApproval,
  type ProviderApprovalDecision,
} from '../domain';
import {
  Button,
  Card,
  CodeBlock,
  Icon,
  Text,
  View,
} from '@lib/tamagui-design-system';
import { TriangleAlert } from '@lib/tamagui-design-system/icons';

export type PendingApprovalCardProps = {
  approval: PendingApproval;
  onRespond: (requestId: string, decision: ProviderApprovalDecision) => void;
};

export function PendingApprovalCard({
  approval,
  onRespond,
}: PendingApprovalCardProps): React.ReactElement {
  const { requestId, title, detail } = approval;
  return (
    <Card tone="warning" testID={`ApprovalCard-${requestId}`} gap="$2">
      <View flexDirection="row" items="center" gap="$1.5">
        <Icon as={TriangleAlert} size={15} color="$yellow9" />
        <Text variant="strong">Approval needed</Text>
      </View>
      <Text>{title}</Text>
      {detail !== undefined ? (
        <CodeBlock code={detail} testID={`ApprovalCardCommand-${requestId}`} />
      ) : null}
      <View flexDirection="row" gap="$2" flexWrap="wrap">
        <Button
          testID={`ApprovalCardAllow-${requestId}`}
          onPress={() => onRespond(requestId, 'accept')}
        >
          Allow once
        </Button>
        <Button
          variant="secondary"
          testID={`ApprovalCardAlways-${requestId}`}
          onPress={() => onRespond(requestId, 'acceptForSession')}
        >
          Always
        </Button>
        <Button
          variant="destructive"
          testID={`ApprovalCardDeny-${requestId}`}
          onPress={() => onRespond(requestId, 'decline')}
        >
          Deny
        </Button>
      </View>
    </Card>
  );
}