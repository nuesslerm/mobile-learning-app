import { fileReviewHeading, foldedRows, type FoldedRow, type SourceCodeLine, type SourceFileReview } from '@lib/skin';
import { Button, Icon, IconButton, Spinner, Text } from '@lib/tamagui-design-system';
import { Check, ChevronDown, ChevronLeft, ChevronRight, UnfoldVertical } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import { memo, useMemo, useState } from 'react';
import { FlatList, ScrollView, useWindowDimensions } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

const CHARACTER_WIDTH = 7.4;

const CodeRow = memo(function CodeRow({ line, gutter }: { line: SourceCodeLine; gutter: number }) {
  const added = line.kind === 'add';
  const deleted = line.kind === 'delete';
  return (
    <XStack bg={added ? '$green3' : deleted ? '$red3' : '$background'}>
      <Text variant="code" width={gutter} text="right" pr="$1.5" color={added || deleted ? '$color' : '$color10'} bg={added ? '$green5' : deleted ? '$red5' : '$background'}>
        {deleted ? line.oldLineNumber : line.newLineNumber}
      </Text>
      <Text variant="code" width={14} text="center" color={added ? '$green10' : '$red10'}>
        {added ? '+' : deleted ? '−' : ' '}
      </Text>
      <Text variant="code">{line.text}</Text>
    </XStack>
  );
});

function FileLines({ lines }: { lines: SourceCodeLine[] }) {
  const [expanded, setExpanded] = useState<ReadonlySet<number>>(new Set());
  const rows = useMemo(() => foldedRows(lines, expanded), [lines, expanded]);
  const { width } = useWindowDimensions();
  const digits = useMemo(() => lines.reduce((max, line) => Math.max(max, String(line.newLineNumber ?? line.oldLineNumber ?? '').length), 1), [lines]);
  const gutter = Math.ceil(14 + digits * CHARACTER_WIDTH);
  const contentWidth = useMemo(() => lines.reduce((max, line) => Math.max(max, line.text.replace(/\t/g, '    ').length), 0) * CHARACTER_WIDTH + gutter + 30, [lines, gutter]);
  return (
    <ScrollView horizontal contentContainerStyle={{ minWidth: '100%' }} style={{ flex: 1 }}>
      <FlatList
        style={{ width: Math.max(width, contentWidth) }}
        data={rows}
        keyExtractor={(row: FoldedRow) => String(row.index)}
        renderItem={({ item }: { item: FoldedRow }) =>
          'fold' in item ? (
            <XStack
              role="button"
              accessible
              aria-label={`Expand ${item.fold} unchanged lines`}
              items="center"
              gap="$2"
              minH={30}
              bg="$blue3"
              pressStyle={{ opacity: 0.7 }}
              cursor="pointer"
              onPress={() => setExpanded((previous) => new Set(previous).add(item.index))}
            >
              <XStack width={gutter + 14} items="center" justify="center" bg="$blue5" self="stretch">
                <Icon as={UnfoldVertical} size={14} color="$blue10" />
              </XStack>
              <Text variant="small" color="$blue10">
                {item.fold} unchanged lines
              </Text>
              {item.scope ? (
                <Text variant="code" color="$color10" numberOfLines={1}>
                  {item.scope}
                </Text>
              ) : null}
            </XStack>
          ) : (
            <CodeRow line={item.line} gutter={gutter} />
          )
        }
      />
    </ScrollView>
  );
}

/** One changed file at a time: its diff with the quiet runs folded, the way to the next file, and on a branch the mark that it was read. */
export function FileReview({ review }: { review: SourceFileReview }) {
  const { bottom } = useSafeAreaInsets();
  const [picker, showPicker] = useState(false);
  const { file, title, subtitle, viewed, advanceLabel, marksViewed, previous, next } = fileReviewHeading(review);
  return (
    <YStack flex={1} bg="$background">
      <XStack items="center" gap="$2" px="$3" minH={52} borderBottomWidth={1} borderColor="$borderColor">
        <IconButton icon={ChevronLeft} variant="secondary" glass={false} size="$3" accessibilityLabel="Back to changed files" pressLog="off" onPress={review.back} testID="skin-review-back" />
        <YStack role="button" accessible aria-label="Select changed file" flex={1} items="center" pressStyle={{ opacity: 0.7 }} cursor="pointer" onPress={() => showPicker(!picker)}>
          <XStack items="center" gap="$1.5">
            <Text variant="strong" numberOfLines={1} shrink={1}>
              {title}
            </Text>
            <Icon as={ChevronDown} size={14} color="$color10" />
          </XStack>
          <Text variant="muted">{subtitle}</Text>
        </YStack>
        <XStack width={36} />
      </XStack>
      {picker ? (
        <FlatList
          data={review.files}
          keyExtractor={(item) => item.path}
          renderItem={({ item, index }) => (
            <XStack
              role="button"
              accessible
              aria-label={item.path}
              px="$4"
              py="$3"
              pressStyle={{ bg: '$color3' }}
              cursor="pointer"
              onPress={() => {
                showPicker(false);
                review.select(index);
              }}
            >
              <Text>{item.path}</Text>
            </XStack>
          )}
        />
      ) : (
        <>
          <XStack items="center" gap="$2" px="$3" py="$2" borderBottomWidth={1} borderColor="$borderColor">
            <Text variant="code" color="$color10" flex={1} numberOfLines={1} ellipsizeMode="middle">
              {file.path}
            </Text>
            {file.additions ? (
              <Text variant="code" color="$green10">
                +{file.additions}
              </Text>
            ) : null}
            {file.deletions ? (
              <Text variant="code" color="$red10">
                −{file.deletions}
              </Text>
            ) : null}
            {review.branch ? (
              <XStack
                role="checkbox"
                accessible
                aria-label="Viewed"
                aria-checked={viewed}
                width={22}
                height={22}
                items="center"
                justify="center"
                rounded="$2"
                borderWidth={1.5}
                borderColor="$color10"
                bg={viewed ? '$color' : 'transparent'}
                cursor="pointer"
                onPress={review.toggleViewed}
              >
                {viewed ? <Icon as={Check} size={14} color="$background" /> : null}
              </XStack>
            ) : null}
          </XStack>
          {review.loading ? (
            <YStack flex={1} items="center" justify="center">
              <Spinner />
            </YStack>
          ) : review.message ? (
            <YStack flex={1} items="center" justify="center" gap="$2" p="$4">
              <Text variant="muted">{review.message}</Text>
              {review.retry ? (
                <Button variant="ghost" glass={false} pressLog="off" onPress={review.retry} testID="skin-review-retry">
                  Retry
                </Button>
              ) : null}
            </YStack>
          ) : review.lines?.length ? (
            <FileLines key={file.path} lines={review.lines} />
          ) : (
            <YStack flex={1} items="center" justify="center">
              <Text variant="muted">No text changes.</Text>
            </YStack>
          )}
          <XStack items="center" gap="$3" px="$3" pt="$2" pb={bottom + 8} borderTopWidth={1} borderColor="$borderColor" bg="$background">
            <IconButton icon={ChevronLeft} variant="secondary" glass={false} size="$3" accessibilityLabel="Previous file" pressLog="off" onPress={previous} testID="skin-review-previous" />
            <Button flex={1} icon={marksViewed ? Check : ChevronRight} pressLog="off" onPress={review.advance} testID="skin-review-advance">
              {advanceLabel}
            </Button>
            <IconButton icon={ChevronRight} variant="secondary" glass={false} size="$3" accessibilityLabel="Next changed file" pressLog="off" onPress={next} testID="skin-review-next" />
          </XStack>
        </>
      )}
    </YStack>
  );
}
