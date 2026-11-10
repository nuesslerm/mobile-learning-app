import { Chip } from '@lib/tamagui-design-system';

import { useThreadDetailStore } from '../../state/threadDetailStore';

export type WorkGroupToggleProps = {
  groupId: string;
  hiddenCount: number;
};

export function WorkGroupToggle({
  groupId,
  hiddenCount,
}: WorkGroupToggleProps): React.ReactElement {
  const expanded = useThreadDetailStore((state) =>
    state.expandedGroupIds.has(groupId)
  );
  const toggleExpandedGroup = useThreadDetailStore(
    (state) => state.toggleExpandedGroup
  );

  return (
    <Chip
      label={
        expanded
          ? `Hide ${hiddenCount} tool calls`
          : `+${hiddenCount} previous tool calls`
      }
      size="sm"
      selected={expanded}
      pressLog="off"
      onPress={() => toggleExpandedGroup(groupId)}
      testID={`WorkGroupToggle-${groupId}`}
    />
  );
}