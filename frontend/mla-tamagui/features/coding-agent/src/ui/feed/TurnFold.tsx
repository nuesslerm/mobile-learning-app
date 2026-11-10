import { useState } from 'react';
import { Icon, Text, View } from '@lib/tamagui-design-system';

import { ChevronDown, ChevronUp } from '../icons';

export type TurnFoldProps = {
  turnId: string;
  title: string;
  children?: React.ReactNode;
};

export function TurnFold({
  turnId,
  title,
  children,
}: TurnFoldProps): React.ReactElement {
  const [expanded, setExpanded] = useState(true);

  return (
    <View testID={`TurnFold-${turnId}`} gap="$1">
      <View
        flexDirection="row"
        items="center"
        justify="space-between"
        py="$1"
        cursor="pointer"
        onPress={() => setExpanded((current) => !current)}
      >
        <Text variant="muted">{title}</Text>
        <Icon as={expanded ? ChevronUp : ChevronDown} size={14} color="$color11" />
      </View>
      {expanded ? children : null}
    </View>
  );
}
