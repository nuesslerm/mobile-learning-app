import { commitAge, fileStatusLetter, fileTotals, sourceControlHeading, type SourceCommit, type SourceControlProps, type SourceFile } from '@lib/skin';
import { Button, Icon, IconButton, Spinner, Text } from '@lib/tamagui-design-system';
import { ChevronDown, ChevronLeft, ChevronRight, GitBranch, GitCommitHorizontal } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import { FlatList } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { SELECTABLE } from '../chat/sessionFiles';
import { BranchSheet } from './branchSheet';
import { FileReview } from './fileReview';
import { PullRequestPanel, SourceControlTabs } from './pullRequest';

const FILES = { paddingBottom: 24 } as const;

function CommitRow({ commit, onPress }: { commit: SourceCommit; onPress: () => void }) {
  return (
    <XStack role="button" accessible aria-label={commit.subject} items="center" gap="$3" px="$4" py="$3" borderBottomWidth={1} borderColor="$borderColor" pressStyle={{ bg: '$color3' }} cursor="pointer" onPress={onPress}>
      <YStack width={8} height={8} rounded={4} bg="$color8" />
      <YStack flex={1} gap="$1">
        <Text numberOfLines={1}>{commit.subject}</Text>
        <Text variant="code" color="$color10" numberOfLines={1}>
          {[commit.shortSha, commit.author, commitAge(commit.authoredAtMs)].filter(Boolean).join(' · ')}
        </Text>
      </YStack>
      <Icon as={ChevronRight} size={16} color="$color10" />
    </XStack>
  );
}

function FileStats({ added, removed }: { added?: number; removed?: number }) {
  return (
    <XStack gap="$1.5">
      {added ? (
        <Text variant="code" color="$green10">
          +{added}
        </Text>
      ) : null}
      {removed ? (
        <Text variant="code" color="$red10">
          −{removed}
        </Text>
      ) : null}
    </XStack>
  );
}

function FileRow({ file, onPress }: { file: SourceFile; onPress: () => void }) {
  const split = file.path.lastIndexOf('/');
  const status = fileStatusLetter(file);
  return (
    <XStack role="button" accessible aria-label={file.path} items="center" gap="$3" px="$4" py="$3" borderBottomWidth={1} borderColor="$borderColor" pressStyle={{ bg: '$color3' }} cursor="pointer" onPress={onPress}>
      <Text variant="code" width={14} text="center" color={status === 'A' ? '$green10' : status === 'D' ? '$red10' : '$color11'}>
        {status}
      </Text>
      <YStack flex={1}>
        <Text numberOfLines={1}>{file.path.slice(split + 1)}</Text>
        {split >= 0 ? (
          <Text variant="muted" numberOfLines={1}>
            {file.path.slice(0, split)}
          </Text>
        ) : null}
      </YStack>
      <FileStats added={file.additions} removed={file.deletions} />
    </XStack>
  );
}

/** A checkout's branch, its commits and what they changed, with the pull request beside the changes and a review of one file at a time. */
export function SourceControl(props: SourceControlProps) {
  const { top } = useSafeAreaInsets();
  const { page, selected, detail, comparison } = props;
  const { title, subtitle, backLabel, branch } = sourceControlHeading(props);
  const notice = props.error ? (
    <YStack items="center" gap="$2" p="$4">
      <Text variant="muted">{props.error}</Text>
      <Button variant="ghost" glass={false} pressLog="off" onPress={props.onRefresh} testID="skin-source-retry">
        Retry
      </Button>
    </YStack>
  ) : props.loading ? (
    <YStack items="center" p="$4">
      <Spinner />
    </YStack>
  ) : undefined;
  const files = page === 'changes' ? comparison?.files : detail?.files;
  const total = fileTotals(detail?.files);
  const ahead = comparison?.ahead;
  const header =
    page === 'changes' ? (
      <>
        <XStack px="$4" pt="$3" pb="$1">
          <XStack role="button" accessible aria-label="Switch Branch" aria-disabled={props.switchingBranch} disabled={props.switchingBranch} items="center" gap="$2" minH={32} maxW="100%" pressStyle={{ opacity: 0.7 }} cursor="pointer" onPress={props.openBranches}>
            <Icon as={GitBranch} size={15} color="$color11" />
            <Text variant="strong" numberOfLines={1} shrink={1}>
              {branch ?? 'No branch'}
            </Text>
            <Icon as={ChevronDown} size={14} color="$color10" />
          </XStack>
        </XStack>
        <XStack role="button" accessible aria-label="Commits" items="center" gap="$3" px="$4" py="$3" borderBottomWidth={1} borderColor="$borderColor" pressStyle={{ bg: '$color3' }} cursor="pointer" onPress={props.openHistory}>
          <Icon as={GitCommitHorizontal} size={16} color="$color11" />
          <Text flex={1}>{ahead === undefined ? 'Commits' : ahead + (ahead === 1 ? ' commit' : ' commits')}</Text>
          <Icon as={ChevronRight} size={16} color="$color10" />
        </XStack>
        {comparison ? (
          <XStack items="center" px="$4" py="$3">
            <Text variant="muted" flex={1}>
              On this branch {comparison.files.length}
              {comparison.limited ? '+' : ''}
            </Text>
            <Text variant="muted">vs {comparison.base}</Text>
          </XStack>
        ) : null}
      </>
    ) : selected ? (
      <YStack>
        <Text variant="large" px="$4" pt="$3">
          {selected.subject}
        </Text>
        {detail?.body ? (
          <Text px="$4" pt="$2" {...SELECTABLE}>
            {detail.body.trim()}
          </Text>
        ) : null}
        <XStack justify="space-between" px="$4" py="$3" borderBottomWidth={1} borderColor="$borderColor">
          <Text>Author</Text>
          <Text color="$color11">{selected.author}</Text>
        </XStack>
        <XStack justify="space-between" px="$4" py="$3">
          <Text>Date</Text>
          <Text color="$color11">{new Date(selected.authoredAtMs).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })}</Text>
        </XStack>
        {detail ? (
          <XStack items="center" px="$4" py="$3">
            <Text variant="muted" flex={1}>
              {detail.initial ? 'Initial commit' : detail.files.length + (detail.limited ? '+ files' : detail.files.length === 1 ? ' file' : ' files')}
            </Text>
            <FileStats added={total?.added} removed={total?.removed} />
          </XStack>
        ) : null}
      </YStack>
    ) : null;
  return (
    <YStack flex={1} bg="$background" pt={top}>
      {/* the lists stay mounted under a file's review, so the way back lands where the reader was */}
      <YStack flex={1} display={props.review ? 'none' : 'flex'}>
        <XStack items="center" px="$3" minH={52} borderBottomWidth={1} borderColor="$borderColor">
          <XStack width={44}>
            <IconButton icon={ChevronLeft} variant="secondary" glass={false} size="$3" accessibilityLabel={backLabel} pressLog="off" onPress={props.onBack} testID="skin-source-back" />
          </XStack>
          <YStack flex={1} items="center">
            <Text variant="strong" role="heading">
              {title}
            </Text>
            {subtitle ? (
              <Text variant="muted" numberOfLines={1}>
                {subtitle}
              </Text>
            ) : null}
          </YStack>
          <XStack width={44} />
        </XStack>
        {page === 'changes' ? <SourceControlTabs selected={props.tab} select={props.selectTab} /> : null}
        {page === 'changes' && props.tab === 'pr' ? <PullRequestPanel panel={props.prPanel} /> : null}
        {page !== 'changes' ? (
          <YStack flex={1} display={page === 'detail' ? 'none' : 'flex'}>
            <FlatList
              key="commits"
              data={props.commits}
              keyExtractor={(item) => item.sha}
              renderItem={({ item }) => <CommitRow commit={item} onPress={() => props.openCommit(item)} />}
              onEndReached={page !== 'commits' || props.error || props.loading || props.loadingMore ? undefined : props.onLoadMore}
              onEndReachedThreshold={0.5}
              refreshing={props.loading && props.commits.length > 0}
              onRefresh={props.onRefresh}
              ListEmptyComponent={
                notice ?? (
                  <Text variant="muted" p="$4">
                    No commits.
                  </Text>
                )
              }
              ListFooterComponent={props.loadingMore ? <Spinner /> : props.commits.length && props.error ? notice : undefined}
            />
          </YStack>
        ) : null}
        {page !== 'commits' && (page !== 'changes' || props.tab === 'changes') ? (
          <FlatList
            key={page}
            data={files ?? []}
            keyExtractor={(file) => file.path}
            contentContainerStyle={FILES}
            renderItem={({ item, index }) => <FileRow file={item} onPress={() => props.openFile(index)} />}
            refreshing={props.loading && Boolean(files)}
            onRefresh={props.onRefresh}
            ListHeaderComponent={header ?? undefined}
            ListFooterComponent={notice}
          />
        ) : null}
      </YStack>
      {props.review ? <FileReview review={props.review} /> : null}
      {props.picker ? <BranchSheet {...props.picker} /> : null}
    </YStack>
  );
}
