import type { SourceControlProps } from '@lib/skin';
import { ChevronRight, GitPullRequest, Link2 } from 'lucide-react-native';
import { useRef, useState } from 'react';
import {
  ActivityIndicator,
  Pressable,
  RefreshControl,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';
import { CheckoutPullRequestSummary } from './CheckoutPullRequestSummary';
import { useChatPalette } from './chatPalette';

type Panel = SourceControlProps['prPanel'];

export function SourceControlTabs({
  selected,
  select,
}: {
  selected: SourceControlProps['tab'];
  select: SourceControlProps['selectTab'];
}) {
  const c = useChatPalette();
  return (
    <View style={[s.tabsContainer, { borderColor: c.borderSubtle }]}>
      <View
        accessibilityRole="tablist"
        style={[s.tabs, { backgroundColor: c.bgPanel }]}
      >
        {(['changes', 'pr'] as const).map((tab) => (
          <Pressable
            key={tab}
            accessibilityRole="tab"
            accessibilityState={{ selected: selected === tab }}
            aria-selected={selected === tab}
            onPress={() => select(tab)}
            style={[s.tab, selected === tab && { backgroundColor: c.bgRaised }]}
          >
            <Text
              style={[
                s.tabText,
                { color: selected === tab ? c.textPrimary : c.textSecondary },
              ]}
            >
              {tab === 'changes' ? 'Changes' : 'Pull request'}
            </Text>
          </Pressable>
        ))}
      </View>
    </View>
  );
}

function LinkPullRequest({
  panel,
  cancel,
}: {
  panel: Panel;
  cancel: () => void;
}) {
  const c = useChatPalette();
  const [input, setInput] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const pending = useRef(false);
  const number = panel.parseReference(input);
  async function submit() {
    if (!number || pending.current) return;
    pending.current = true;
    setBusy(true);
    setError('');
    try {
      await panel.link(number);
      cancel();
    } catch {
      setError(
        'Could not link this pull request. Check the number and try again.'
      );
    } finally {
      pending.current = false;
      setBusy(false);
    }
  }
  return (
    <View style={s.form}>
      <View style={s.heading}>
        <Text style={[s.headingText, { color: c.textPrimary }]}>
          Link existing pull request
        </Text>
        <Pressable accessibilityRole="button" disabled={busy} onPress={cancel}>
          <Text style={{ color: c.textSecondary }}>Cancel</Text>
        </Pressable>
      </View>
      <Text style={{ color: c.textSecondary }}>PR number or GitHub URL</Text>
      <TextInput
        editable={!busy}
        accessibilityLabel="PR number or GitHub URL"
        autoCapitalize="none"
        autoCorrect={false}
        placeholder="#123 or https://github.com/owner/repo/pull/123"
        placeholderTextColor={c.textMuted}
        value={input}
        onChangeText={setInput}
        onSubmitEditing={() => void submit()}
        style={[s.field, { color: c.textPrimary, borderColor: c.borderSubtle }]}
      />
      {!!error && (
        <Text role="alert" style={{ color: c.statusRed }}>
          {error}
        </Text>
      )}
      <Pressable
        accessibilityRole="button"
        disabled={!number || busy}
        onPress={() => void submit()}
        style={[
          s.submit,
          {
            backgroundColor: c.surfaceBright,
            opacity: !number || busy ? 0.5 : 1,
          },
        ]}
      >
        {busy ? (
          <ActivityIndicator color={c.bgBase} />
        ) : (
          <Text style={{ color: c.bgBase, fontWeight: '600' }}>
            {number ? 'Link #' + number : 'Link pull request'}
          </Text>
        )}
      </Pressable>
    </View>
  );
}

export function PullRequestPanel({ panel }: { panel: Panel }) {
  const c = useChatPalette();
  const [linking, setLinking] = useState(false);
  const { result, error } = panel;
  if (error || !result)
    return (
      <View style={s.state}>
        {!error && <ActivityIndicator color={c.textMuted} />}
        <Text style={{ color: c.textSecondary }}>
          {error ?? 'Loading pull request…'}
        </Text>
        {error && (
          <Pressable accessibilityRole="button" onPress={panel.refresh}>
            <Text style={{ color: c.textPrimary }}>Retry</Text>
          </Pressable>
        )}
      </View>
    );
  const pr = result.pullRequest;
  return (
    <ScrollView
      keyboardShouldPersistTaps="handled"
      contentContainerStyle={s.body}
      refreshControl={
        <RefreshControl refreshing={false} onRefresh={panel.refresh} />
      }
    >
      {pr ? (
        <CheckoutPullRequestSummary pr={pr} open={panel.open} />
      ) : linking ? (
        <LinkPullRequest panel={panel} cancel={() => setLinking(false)} />
      ) : (
        <>
          <View style={s.empty}>
            <GitPullRequest size={28} strokeWidth={1.6} color={c.textMuted} />
            <Text style={[s.headingText, { color: c.textPrimary }]}>
              No pull request yet
            </Text>
            <Text style={[s.emptyBody, { color: c.textSecondary }]}>
              {(result.branch || 'The current branch') +
                ' is not linked to an open PR.'}
            </Text>
          </View>
          <Pressable
            accessibilityRole="button"
            onPress={() => setLinking(true)}
            style={[s.row, { backgroundColor: c.bgRaised }]}
          >
            <Link2 size={18} color={c.textSecondary} />
            <Text style={[s.grow, { color: c.textPrimary }]}>
              Link an existing pull request
            </Text>
            <ChevronRight size={16} color={c.textMuted} />
          </Pressable>
        </>
      )}
    </ScrollView>
  );
}
const s = StyleSheet.create({
  tabsContainer: {
    paddingHorizontal: 12,
    paddingTop: 8,
    paddingBottom: 8,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  tabs: { padding: 3, borderRadius: 12, flexDirection: 'row' },
  tab: {
    flex: 1,
    minHeight: 34,
    alignItems: 'center',
    justifyContent: 'center',
    borderRadius: 9,
  },
  tabText: { fontSize: 13, fontWeight: '600' },
  state: { flex: 1, alignItems: 'center', justifyContent: 'center', gap: 8 },
  body: { padding: 16, gap: 8 },
  empty: {
    alignItems: 'center',
    paddingHorizontal: 16,
    paddingVertical: 24,
    gap: 8,
  },
  emptyBody: { textAlign: 'center', lineHeight: 20 },
  heading: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
  },
  headingText: { fontSize: 17, fontWeight: '600' },
  row: {
    minHeight: 48,
    paddingHorizontal: 16,
    borderRadius: 16,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
  },
  grow: { flex: 1 },
  form: { gap: 10 },
  field: {
    minHeight: 44,
    borderRadius: 12,
    borderWidth: 1,
    paddingHorizontal: 12,
  },
  submit: {
    minHeight: 42,
    alignItems: 'center',
    justifyContent: 'center',
    borderRadius: 12,
  },
});
