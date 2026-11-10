import { useState } from 'react';
import {
  parseUnifiedDiff,
  type WorkLogEntry,
  type WorkLogKind,
  type WorkLogStatus,
} from '../../domain';
import {
  CodeBlock,
  DiffView,
  Icon,
  Spinner,
  Text,
  View,
} from '@lib/tamagui-design-system';
import {
  ChevronDown,
  ChevronUp,
  CircleCheck,
  CircleX,
  Code,
  Eye,
  FilePen,
  Globe,
  Plug,
  Terminal,
  Wrench,
} from '@lib/tamagui-design-system/icons';

const KIND_ICON: Record<WorkLogKind, typeof Terminal> = {
  command: Terminal,
  file_change: FilePen,
  file_read: Eye,
  web_search: Globe,
  mcp: Plug,
  tool: Wrench,
  diff: Code,
};

export type WorkLogGroupProps = {
  groupId: string;
  entries: readonly WorkLogEntry[];
};

export function WorkLogGroup({
  groupId,
  entries,
}: WorkLogGroupProps): React.ReactElement | null {
  if (entries.length === 0) {
    return null;
  }
  return (
    <View testID={`ToolGroup-${groupId}`} gap="$1">
      {entries.map((entry) => (
        <WorkLogRow key={entry.id} entry={entry} />
      ))}
    </View>
  );
}

function WorkLogRow({ entry }: { entry: WorkLogEntry }): React.ReactElement {
  const [expanded, setExpanded] = useState(false);
  const canExpand = entry.output !== undefined || entry.diff !== undefined;

  return (
    <View testID={`ToolRow-${entry.id}`} gap="$1">
      <View testID={`WorkLogRow-${entry.id}`} gap="$1">
        <View
          flexDirection="row"
          items="center"
          gap="$2"
          px="$1"
          py="$1"
          cursor={canExpand ? 'pointer' : undefined}
          onPress={() => {
            if (canExpand) {
              setExpanded((current) => !current);
            }
          }}
        >
          <Icon as={KIND_ICON[entry.kind]} size={15} color="$color11" />
          <Text size="$2" numberOfLines={1} flex={1}>
            {entry.title}
          </Text>
          <StatusGlyph status={entry.status} />
          {canExpand ? (
            <Icon
              as={expanded ? ChevronUp : ChevronDown}
              size={14}
              color="$color11"
            />
          ) : null}
        </View>
        {expanded ? (
          <View pl="$5" gap="$1.5">
            {entry.output !== undefined ? (
              <CodeBlock
                code={entry.output}
                title={entry.title}
                testID={`ToolRowDetail-${entry.id}`}
              />
            ) : null}
            {entry.diff !== undefined ? (
              <DiffView
                files={parseUnifiedDiff(entry.diff)}
                testID={`ToolRowDiff-${entry.id}`}
              />
            ) : null}
          </View>
        ) : null}
      </View>
    </View>
  );
}

function StatusGlyph({
  status,
}: {
  status: WorkLogStatus;
}): React.ReactElement {
  if (status === 'running') {
    return <Spinner size="small" />;
  }
  return (
    <Icon
      as={status === 'done' ? CircleCheck : CircleX}
      size={14}
      color={status === 'done' ? '$green9' : '$red9'}
    />
  );
}
