import { pullRequestFacts, type SourceControlProps } from '@lib/skin';
import {
  ArrowRight,
  CheckCircle2,
  ChevronRight,
  Circle,
  GitMerge,
  GitPullRequest,
} from 'lucide-react-native';
import { Platform, Pressable, StyleSheet, Text, View } from 'react-native';
import { useChatPalette } from './chatPalette';

type PullRequest = NonNullable<
  NonNullable<SourceControlProps['prPanel']['result']>['pullRequest']
>;

export function CheckoutPullRequestSummary({
  pr,
  open,
}: {
  pr: PullRequest;
  open: (url: string) => void;
}) {
  const c = useChatPalette();
  const { checks } = pr;
  const facts = pullRequestFacts(pr);
  const state = facts.state;
  const color =
    pr.state === 'merged'
      ? '#8250df'
      : pr.state === 'closed'
        ? c.statusRed
        : pr.draft
          ? c.textSecondary
          : c.statusGreen;
  const checksLabel = facts.checks;
  const muted = { color: c.textSecondary };
  const ink = { color: c.textPrimary };
  const RefIcon = pr.state === 'merged' ? GitMerge : GitPullRequest;
  return (
    <>
      <View style={s.metas}>
        <View style={[s.badge, { borderColor: color }]}>
          <RefIcon size={12} strokeWidth={2} color={color} />
          <Text style={[s.badgeText, { color }]}>{state}</Text>
        </View>
        <Text style={[s.number, ink]}>#{pr.number}</Text>
        <Text style={[s.meta, muted]} numberOfLines={1}>
          {pr.author}
        </Text>
      </View>
      <Pressable
        accessibilityRole="link"
        accessibilityLabel={'Open pull request #' + pr.number + ' on GitHub'}
        onPress={() => open(pr.url)}
      >
        <Text style={[s.title, ink]}>{pr.title}</Text>
      </Pressable>
      <View style={s.metas}>
        <Text style={[s.ref, muted, { backgroundColor: c.bgRaised }]}>
          {pr.head}
        </Text>
        <ArrowRight size={12} color={c.textMuted} />
        <Text style={[s.ref, muted, { backgroundColor: c.bgRaised }]}>
          {pr.base}
        </Text>
      </View>
      <View style={[s.group, { backgroundColor: c.bgGroup }]}>
        <Pressable
          accessibilityRole="link"
          onPress={() => open(pr.url + '/checks')}
          style={[s.row, { borderColor: c.borderSubtle }]}
        >
          <Circle
            size={18}
            color={
              checks?.failing
                ? c.statusRed
                : checks?.pending
                  ? c.statusAmber
                  : c.textSecondary
            }
          />
          <View style={s.grow}>
            <Text style={ink}>{checksLabel}</Text>
            {!!checks?.pending && (
              <Text style={muted}>{checks.passing} passed so far</Text>
            )}
          </View>
          <ChevronRight size={16} color={c.textMuted} />
        </Pressable>
        {pr.reviewDecision && (
          <Text style={[s.fact, ink]}>
            {facts.review}
          </Text>
        )}
        <View style={[s.row, { borderColor: c.borderSubtle }]}>
          <CheckCircle2
            size={18}
            color={
              pr.mergeable === 'mergeable' ? c.statusGreen : c.textSecondary
            }
          />
          <Text style={ink}>
            {facts.merge}
          </Text>
        </View>
      </View>
      {pr.files !== null && (
        <Pressable
          accessibilityRole="link"
          accessibilityLabel="View changed files on GitHub"
          onPress={() => open(pr.url + '/files')}
          style={s.row}
        >
          <Text style={[s.grow, ink]}>
            {facts.filesChanged}
          </Text>
          {pr.additions !== null && (
            <Text style={{ color: c.statusGreen }}>+{pr.additions}</Text>
          )}
          {pr.deletions !== null && (
            <Text style={{ color: c.statusRed }}>−{pr.deletions}</Text>
          )}
        </Pressable>
      )}
      <Text style={muted}>Description</Text>
      <Text selectable style={pr.description ? ink : muted}>
        {pr.description || 'No description'}
      </Text>
    </>
  );
}
const s = StyleSheet.create({
  metas: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    flexWrap: 'wrap',
  },
  badge: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 4,
    borderWidth: StyleSheet.hairlineWidth,
    borderRadius: 999,
    paddingHorizontal: 8,
    paddingVertical: 1,
  },
  badgeText: { fontSize: 12, fontWeight: '600' },
  number: { fontSize: 13, fontWeight: '600' },
  meta: { fontSize: 12, flexShrink: 1 },
  title: { fontSize: 18, fontWeight: '600', lineHeight: 24 },
  ref: {
    fontSize: 12,
    fontFamily: Platform.OS === 'ios' ? 'Menlo' : 'monospace',
    paddingHorizontal: 4,
    borderRadius: 4,
    overflow: 'hidden',
  },
  group: { marginVertical: 4, borderRadius: 16 },
  row: {
    minHeight: 48,
    paddingHorizontal: 16,
    paddingVertical: 8,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderColor: 'transparent',
  },
  grow: { flex: 1 },
  fact: { paddingHorizontal: 16, paddingVertical: 12 },
});
