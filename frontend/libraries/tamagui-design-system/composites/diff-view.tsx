import { Logger } from '@lib/logging';
import { useState } from 'react';
import type { ReactElement } from 'react';
import { ScrollView } from 'react-native';

import { ChevronDown, ChevronRight } from '../icons';
import { XStack, YStack } from '../primitives';
import { Badge } from '../ui/badge';
import { Icon } from '../ui/icon';
import { Text } from '../ui/text';

export type ParsedDiffLine = {
  kind: 'add' | 'del' | 'ctx';
  text: string;
  oldNo?: number;
  newNo?: number;
};

export type ParsedDiffHunk = {
  header: string;
  lines: ParsedDiffLine[];
};

export type ParsedDiffFile = {
  path: string;
  oldPath?: string;
  additions: number;
  deletions: number;
  hunks: ParsedDiffHunk[];
};

export type DiffViewProps = {
  files: ParsedDiffFile[];
  testID?: string;
};

/**
 * Per-file diff card.
 *
 * Mirrors the file part of a `git diff` output: a header row with the path and
 * `+N` / `-M` count badges, a collapse toggle, hunk headers (e.g.
 * `@@ -12,7 +12,9 @@`), then a column for the old line number, a column for the
 * new line number, a `+`/`-`/space marker, and the line text in the monospace
 * `code` Text variant.
 *
 * Backgrounds use the Tamagui color tokens `$green3` / `$red3` for added and
 * deleted lines -- both differ from the card background (`$color2`) in light
 * and dark themes, so the diff rows stay readable when the page is themed.
 * No syntax highlighting: per the contract, this composite ships the raw text.
 *
 * The `ParsedDiffFile` shape is defined locally on purpose so the design
 * system does not depend on the feature package that produces the diff.
 */
export function DiffView({ files, testID }: DiffViewProps): ReactElement {
  return (
    <YStack testID={testID} gap="$2">
      {files.map((file, i) => (
        <DiffFile key={`${file.path}-${i}`} index={i} file={file} />
      ))}
    </YStack>
  );
}

function DiffFile({ index, file }: { index: number; file: ParsedDiffFile }) {
  const [collapsed, setCollapsed] = useState(false);

  const handleToggle = () => {
    const next = !collapsed;
    setCollapsed(next);
    Logger.info('ds.diffview.toggle', {
      path: file.path,
      index,
      collapsed: next,
    });
  };

  return (
    <YStack
      rounded="$3"
      borderWidth={1}
      borderColor="$borderColor"
      bg="$color2"
      gap={0}
      overflow="hidden"
    >
      <DiffFileHeader
        index={index}
        path={file.path}
        additions={file.additions}
        deletions={file.deletions}
        collapsed={collapsed}
        onToggle={handleToggle}
      />
      {collapsed ? null : (
        <YStack>
          {file.hunks.map((hunk, hunkIdx) => (
            <DiffHunk key={`${hunk.header}-${hunkIdx}`} hunk={hunk} />
          ))}
        </YStack>
      )}
    </YStack>
  );
}

function DiffFileHeader({
  index,
  path,
  additions,
  deletions,
  collapsed,
  onToggle,
}: {
  index: number;
  path: string;
  additions: number;
  deletions: number;
  collapsed: boolean;
  onToggle: () => void;
}): ReactElement {
  return (
    <XStack
      testID={`DiffFile-${index}-Toggle`}
      items="center"
      gap="$2"
      px="$3"
      py="$2"
      bg="$color3"
      borderBottomWidth={collapsed ? 0 : 1}
      borderColor="$borderColor"
      cursor="pointer"
      pressStyle={{ opacity: 0.7 }}
      onPress={onToggle}
    >
      <Icon
        as={collapsed ? ChevronRight : ChevronDown}
        size={14}
        color="$color11"
      />
      <Text size="$2" color="$color" numberOfLines={1} flex={1}>
        {path}
      </Text>
      <Badge variant="success">{`+${additions}`}</Badge>
      <Badge variant="destructive">{`−${deletions}`}</Badge>
    </XStack>
  );
}

function DiffHunk({ hunk }: { hunk: ParsedDiffHunk }): ReactElement {
  return (
    <YStack>
      <XStack
        bg="$color3"
        px="$3"
        py="$1.5"
        borderTopWidth={1}
        borderColor="$borderColor"
      >
        <Text size="$1" color="$color11" numberOfLines={1}>
          {hunk.header}
        </Text>
      </XStack>
      <ScrollView horizontal showsHorizontalScrollIndicator={false}>
        <YStack>
          {hunk.lines.map((line, i) => (
            <DiffLine key={i} line={line} />
          ))}
        </YStack>
      </ScrollView>
    </YStack>
  );
}

function DiffLine({ line }: { line: ParsedDiffLine }): ReactElement {
  const marker = line.kind === 'add' ? '+' : line.kind === 'del' ? '−' : ' ';
  return (
    <XStack
      {...(line.kind === 'add' && { bg: '$green3' })}
      {...(line.kind === 'del' && { bg: '$red3' })}
    >
      <DiffGutter no={line.oldNo} />
      <DiffGutter no={line.newNo} />
      <XStack width={20} px="$1" items="center" justify="center">
        <Text variant="code" size="$1">
          {marker}
        </Text>
      </XStack>
      <Text variant="code" size="$1" px="$2" py="$0.5">
        {line.text}
      </Text>
    </XStack>
  );
}

function DiffGutter({ no }: { no?: number }): ReactElement {
  return (
    <XStack width={50} px="$1" items="flex-end" justify="flex-end">
      <Text variant="code" size="$1" color="$color11">
        {no ?? ''}
      </Text>
    </XStack>
  );
}
