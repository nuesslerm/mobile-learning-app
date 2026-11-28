import { pullRequestFacts, useLinkPullRequest, type SourceControlProps, type SourcePullRequest } from '@lib/skin';
import { Button, Icon, InsetGroup, ListItem, Spinner, Text, TextField } from '@lib/tamagui-design-system';
import { ArrowRight, Circle, CircleCheck, GitMerge, GitPullRequest, Link2 } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import { useState } from 'react';
import { RefreshControl, ScrollView } from 'react-native';

import { SELECTABLE } from '../chat/sessionFiles';

type Panel = SourceControlProps['prPanel'];

const BODY = { padding: 16, gap: 14 } as const;
// the app's themes have no purple: merged is the colour it has on GitHub
const STATE_INK = { open: '$green9', draft: '$color11', merged: '#8250df', closed: '$red9' } as const;
const CHECK_INK = { failing: '$red9', pending: '$yellow9', quiet: '$color11' } as const;

/** The screen's two views of a branch, as the app's segmented choice. */
export function SourceControlTabs({ selected, select }: { selected: SourceControlProps['tab']; select: SourceControlProps['selectTab'] }) {
  return (
    <YStack px="$3" py="$2" borderBottomWidth={1} borderColor="$borderColor">
      <XStack role="tablist" p="$1" gap="$1" rounded="$4" bg="$color3">
        {(['changes', 'pr'] as const).map((tab) => (
          <XStack
            key={tab}
            role="tab"
            accessible
            aria-selected={selected === tab}
            flex={1}
            minH={34}
            items="center"
            justify="center"
            rounded="$3"
            bg={selected === tab ? '$background' : 'transparent'}
            pressStyle={{ opacity: 0.7 }}
            cursor="pointer"
            onPress={() => select(tab)}
          >
            <Text variant="small" color={selected === tab ? '$color' : '$color11'}>
              {tab === 'changes' ? 'Changes' : 'Pull request'}
            </Text>
          </XStack>
        ))}
      </XStack>
    </YStack>
  );
}

function LinkPullRequest({ panel, cancel }: { panel: Panel; cancel: () => void }) {
  const { input, setInput, busy, error, number, submit, submitLabel } = useLinkPullRequest(panel, cancel);
  return (
    <YStack gap="$3">
      <XStack items="center">
        <Text variant="large" flex={1}>
          Link existing pull request
        </Text>
        <Button variant="ghost" glass={false} size="$3" disabled={busy} pressLog="off" onPress={cancel} testID="skin-pr-link-cancel">
          Cancel
        </Button>
      </XStack>
      <Text variant="muted">PR number or GitHub URL</Text>
      <TextField
        testID="skin-pr-link-field"
        aria-label="PR number or GitHub URL"
        disabled={busy}
        autoCapitalize="none"
        autoCorrect={false}
        placeholder="#123 or https://github.com/owner/repo/pull/123"
        placeholderTextColor="$color10"
        value={input}
        onChangeText={setInput}
        onSubmitEditing={() => void submit()}
      />
      {error ? (
        <Text role="alert" color="$red10">
          {error}
        </Text>
      ) : null}
      <Button disabled={!number || busy} pressLog="off" onPress={() => void submit()} testID="skin-pr-link-submit">
        {busy ? 'Linking…' : submitLabel}
      </Button>
    </YStack>
  );
}

function PullRequestSummary({ pr, open }: { pr: SourcePullRequest; open: (url: string) => void }) {
  const facts = pullRequestFacts(pr);
  const ink = STATE_INK[facts.tone];
  return (
    <>
      <XStack items="center" gap="$2">
        <XStack items="center" gap="$1" px="$2" py="$1" rounded={9999} borderWidth={1} borderColor={ink}>
          <Icon as={pr.state === 'merged' ? GitMerge : GitPullRequest} size={12} color={ink} />
          <Text variant="small" color={ink}>
            {facts.state}
          </Text>
        </XStack>
        <Text variant="strong">#{pr.number}</Text>
        <Text variant="muted" numberOfLines={1} flex={1}>
          {pr.author}
        </Text>
      </XStack>
      <YStack role="link" accessible aria-label={`Open pull request #${pr.number} on GitHub`} pressStyle={{ opacity: 0.7 }} cursor="pointer" onPress={() => open(pr.url)}>
        <Text variant="large">{pr.title}</Text>
      </YStack>
      <XStack items="center" gap="$2">
        <Text variant="code" color="$color11" px="$2" py="$1" rounded="$2" bg="$color3">
          {pr.head}
        </Text>
        <Icon as={ArrowRight} size={12} color="$color10" />
        <Text variant="code" color="$color11" px="$2" py="$1" rounded="$2" bg="$color3">
          {pr.base}
        </Text>
      </XStack>
      <InsetGroup inset={false} testID="skin-pr-facts">
        <XStack role="link" accessible items="center" gap="$3" px="$4" py="$3" pressStyle={{ bg: '$color3' }} cursor="pointer" onPress={() => open(pr.url + '/checks')}>
          <Icon as={Circle} size={18} color={CHECK_INK[facts.checksTone]} />
          <YStack flex={1}>
            <Text>{facts.checks}</Text>
            {pr.checks?.pending ? <Text variant="muted">{pr.checks.passing} passed so far</Text> : null}
          </YStack>
        </XStack>
        {facts.review ? (
          <XStack px="$4" py="$3">
            <Text>{facts.review}</Text>
          </XStack>
        ) : null}
        <XStack items="center" gap="$3" px="$4" py="$3">
          <Icon as={CircleCheck} size={18} color={pr.mergeable === 'mergeable' ? '$green9' : '$color11'} />
          <Text flex={1}>{facts.merge}</Text>
        </XStack>
        {facts.filesChanged ? (
          <XStack role="link" accessible aria-label="View changed files on GitHub" items="center" gap="$2" px="$4" py="$3" pressStyle={{ bg: '$color3' }} cursor="pointer" onPress={() => open(pr.url + '/files')}>
            <Text flex={1}>{facts.filesChanged}</Text>
            {pr.additions === null ? null : <Text color="$green9">+{pr.additions}</Text>}
            {pr.deletions === null ? null : <Text color="$red9">−{pr.deletions}</Text>}
          </XStack>
        ) : null}
      </InsetGroup>
      <Text variant="muted">Description</Text>
      <Text color={pr.description ? '$color' : '$color11'} {...SELECTABLE}>
        {pr.description || 'No description'}
      </Text>
    </>
  );
}

/** The branch's pull request: its state and checks, or the way to link one that already exists. */
export function PullRequestPanel({ panel }: { panel: Panel }) {
  const [linking, setLinking] = useState(false);
  const { result, error } = panel;
  if (error || !result)
    return (
      <YStack flex={1} items="center" justify="center" gap="$2" p="$4">
        {error ? null : <Spinner />}
        <Text variant="muted">{error ?? 'Loading pull request…'}</Text>
        {error ? (
          <Button variant="ghost" glass={false} pressLog="off" onPress={panel.refresh} testID="skin-pr-retry">
            Retry
          </Button>
        ) : null}
      </YStack>
    );
  const pr = result.pullRequest;
  return (
    <ScrollView keyboardShouldPersistTaps="handled" contentContainerStyle={BODY} refreshControl={<RefreshControl refreshing={false} onRefresh={panel.refresh} />}>
      {pr ? (
        <PullRequestSummary pr={pr} open={panel.open} />
      ) : linking ? (
        <LinkPullRequest panel={panel} cancel={() => setLinking(false)} />
      ) : (
        <>
          <YStack items="center" gap="$2" py="$6">
            <Icon as={GitPullRequest} size={28} color="$color10" />
            <Text variant="large">No pull request yet</Text>
            <Text variant="muted" text="center">
              {(result.branch || 'The current branch') + ' is not linked to an open PR.'}
            </Text>
          </YStack>
          <InsetGroup inset={false} testID="skin-pr-link">
            <ListItem variant="grouped" showChevron pressLog="off" accessibilityLabel="Link an existing pull request" onPress={() => setLinking(true)} testID="skin-pr-link-open">
              <XStack items="center" gap="$3">
                <Icon as={Link2} size={18} color="$color11" />
                <Text flex={1}>Link an existing pull request</Text>
              </XStack>
            </ListItem>
          </InsetGroup>
        </>
      )}
    </ScrollView>
  );
}
