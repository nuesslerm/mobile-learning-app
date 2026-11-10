import {
  Button,
  Card,
  Icon,
  Text,
  View,
} from '@lib/tamagui-design-system';
import { Plug, RefreshCw } from '@lib/tamagui-design-system/icons';

import type { OrchestrationGatewayStatus } from '../data/orchestrationGateway';
import { useGatewayStore } from '../state/gatewayStore';

const STATUS_LABEL: Partial<Record<OrchestrationGatewayStatus, string>> = {
  connecting: 'Connecting to the coding agent…',
  reconnecting: 'Reconnecting to the coding agent…',
  closed: 'The coding agent is disconnected.',
};

export function ConnectionBanner(): React.ReactElement | null {
  const status = useGatewayStore((state) => state.status);
  const connect = useGatewayStore((state) => state.connect);
  const label = STATUS_LABEL[status];
  if (label === undefined) {
    return null;
  }
  return (
    <Card tone="warning" testID="ConnectionBanner" gap="$2">
      <View flexDirection="row" items="center" gap="$1.5">
        <Icon as={Plug} size={15} color="$yellow9" />
        <Text size="$3">{label}</Text>
      </View>
      <Button
        variant="secondary"
        icon={RefreshCw}
        testID="ConnectionBannerRetry"
        onPress={() => void connect()}
      >
        Retry
      </Button>
    </Card>
  );
}