import type {
  SourceControlProps,
  SourceCommit,
  SourceFile,
} from '@lib/agent-ui-contracts';
import GitBranch from 'lucide-react-native/dist/esm/icons/git-branch.mjs';
import GitCommit from 'lucide-react-native/dist/esm/icons/git-commit-horizontal.mjs';
import {
  ActivityIndicator,
  FlatList,
  Platform,
  Pressable,
  StyleSheet,
  Text,
  View,
} from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { BackIcon, ChevronRightIcon } from './homeIcons';
import { useChatPalette } from './chatPalette';

const mono = Platform.OS === 'ios' ? 'Menlo' : 'monospace';

function age(timestamp: number) {
  const minutes = Math.floor((Date.now() - timestamp) / 60_000);
  if (minutes < 1) return 'just now';
  if (minutes < 60) return minutes + 'm';
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return hours + 'h';
  const days = Math.floor(hours / 24);
  if (days < 30) return days + 'd';
  const months = Math.floor(days / 30);
  return months < 12 ? months + 'mo' : Math.floor(months / 12) + 'y';
}

function CommitRow({
  commit,
  first,
  last,
  onPress,
}: {
  commit: SourceCommit;
  first: boolean;
  last: boolean;
  onPress: () => void;
}) {
  const c = useChatPalette();
  const line = { backgroundColor: c.translucentFill };
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={commit.subject}
      onPress={onPress}
      style={s.commit}
    >
      <View style={s.timeline}>
        <View style={[s.line, !first && line]} />
        <View style={[s.dot, { backgroundColor: c.textMuted }]} />
        <View style={[s.line, !last && line]} />
      </View>
      <View style={[s.commitBody, { borderColor: c.borderSubtle }]}>
        <View style={s.flex}>
          <Text
            style={{ fontSize: 14, color: c.textPrimary }}
            numberOfLines={1}
          >
            {commit.subject}
          </Text>
          <Text style={[s.meta, { color: c.textMuted }]} numberOfLines={1}>
            {[commit.shortSha, commit.author, age(commit.authoredAtMs)]
              .filter(Boolean)
              .join(' · ')}
          </Text>
        </View>
        <ChevronRightIcon size={16} color={c.textMuted} strokeWidth={2.2} />
      </View>
    </Pressable>
  );
}

function FileStats({ added, removed }: { added?: number; removed?: number }) {
  const c = useChatPalette();
  return (
    <Text style={s.stats}>
      {!!added && <Text style={{ color: c.statusGreen }}>+{added}</Text>}
      {!!added && !!removed && ' '}
      {!!removed && <Text style={{ color: c.statusRed }}>−{removed}</Text>}
    </Text>
  );
}

function FileRow({ file }: { file: SourceFile }) {
  const c = useChatPalette();
  const pos = file.path.lastIndexOf('/');
  const status =
    file.status === 'type_changed' ? 'T' : file.status[0]?.toUpperCase();
  return (
    <View style={[s.file, { borderColor: c.borderSubtle }]}>
      <Text
        style={[
          s.stats,
          {
            color:
              status === 'A'
                ? c.statusGreen
                : status === 'D'
                  ? c.statusRed
                  : c.textSecondary,
          },
        ]}
      >
        {status}
      </Text>
      <View style={s.flex}>
        <Text numberOfLines={1} style={{ fontSize: 14, color: c.textPrimary }}>
          {file.path.slice(pos + 1)}
        </Text>
        {pos >= 0 && (
          <Text
            numberOfLines={1}
            style={{ fontSize: 12, color: c.textSecondary }}
          >
            {file.path.slice(0, pos)}
          </Text>
        )}
      </View>
      <FileStats added={file.additions} removed={file.deletions} />
    </View>
  );
}

export function SourceControl(props: SourceControlProps) {
  const c = useChatPalette();
  const { page, selected, detail, comparison } = props;
  const title =
    page === 'changes'
      ? 'Source Control'
      : page === 'commits'
        ? 'Commits'
        : 'Commit';
  const branch = props.branch ?? props.head?.slice(0, 7);
  const subtitle = page === 'detail' ? selected?.shortSha : branch;
  const text = { color: c.textPrimary };
  const muted = { color: c.textSecondary };
  const backLabel =
    page === 'detail'
      ? 'Back to commits'
      : page === 'commits'
        ? 'Back to source control'
        : 'Back to workspaces';
  const notice = props.error ? (
    <View style={s.notice}>
      <Text style={muted}>{props.error}</Text>
      <Pressable accessibilityRole="button" onPress={props.onRefresh}>
        <Text style={text}>Retry</Text>
      </Pressable>
    </View>
  ) : props.loading ? (
    <ActivityIndicator style={s.notice} color={c.textSecondary} />
  ) : undefined;
  const files = page === 'changes' ? comparison?.files : detail?.files;
  const total = detail?.files.reduce(
    (values, f) => ({
      added: values.added + (f.additions ?? 0),
      removed: values.removed + (f.deletions ?? 0),
    }),
    { added: 0, removed: 0 }
  );
  const ahead = comparison?.ahead;
  const header =
    page === 'changes' ? (
      <>
        <View style={s.summaryRow}>
          <GitBranch size={16} color={c.textSecondary} />
          <Text style={text}>{branch ?? 'No branch'}</Text>
        </View>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Commits"
          onPress={props.openHistory}
          style={[
            s.summaryRow,
            { borderBottomWidth: 1, borderColor: c.borderSubtle },
          ]}
        >
          <GitCommit size={16} color={c.textSecondary} />
          <Text style={[s.flex, text]}>
            {ahead === undefined
              ? 'Commits'
              : ahead + (ahead === 1 ? ' commit' : ' commits')}
          </Text>
          <ChevronRightIcon size={16} color={c.textMuted} />
        </Pressable>
        {comparison && (
          <View style={s.summaryRow}>
            <Text style={[s.flex, muted]}>
              On this branch {comparison.files.length}
              {comparison.limited ? '+' : ''}
            </Text>
            <Text style={muted}>vs {comparison.base}</Text>
          </View>
        )}
      </>
    ) : (
      selected && (
        <>
          <Text style={[s.subject, text]}>{selected.subject}</Text>
          {!!detail?.body && (
            <Text selectable style={[s.body, text]}>
              {detail.body.trim()}
            </Text>
          )}
          <View style={[s.fact, { borderColor: c.borderSubtle }]}>
            <Text style={text}>Author</Text>
            <Text style={muted}>{selected.author}</Text>
          </View>
          <View style={s.fact}>
            <Text style={text}>Date</Text>
            <Text style={muted}>
              {new Date(selected.authoredAtMs).toLocaleString(undefined, {
                dateStyle: 'medium',
                timeStyle: 'short',
              })}
            </Text>
          </View>
          {detail && (
            <View style={s.summaryRow}>
              <Text style={[s.flex, muted]}>
                {detail.initial
                  ? 'Initial commit'
                  : detail.files.length +
                    (detail.limited
                      ? '+ files'
                      : detail.files.length === 1
                        ? ' file'
                        : ' files')}
              </Text>
              <FileStats added={total?.added} removed={total?.removed} />
            </View>
          )}
        </>
      )
    );
  return (
    <SafeAreaView
      style={[s.screen, { backgroundColor: c.bgBase }]}
      edges={['top', 'left', 'right']}
    >
      <View style={[s.nav, { borderColor: c.borderSubtle }]}>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={backLabel}
          onPress={props.onBack}
          style={[s.back, { backgroundColor: c.bgRaised }]}
        >
          <BackIcon size={20} color={c.textPrimary} />
        </Pressable>
        <View style={s.heading}>
          <Text accessibilityRole="header" style={[s.title, text]}>
            {title}
          </Text>
          {subtitle && <Text style={[s.subtitle, muted]}>{subtitle}</Text>}
        </View>
      </View>
      {page !== 'changes' && (
        <View style={page === 'detail' ? { display: 'none' } : s.flex}>
          <FlatList
            key="commits"
            data={props.commits}
            keyExtractor={(item) => item.sha}
            renderItem={({ item, index }) => (
              <CommitRow
                commit={item}
                first={index === 0}
                last={index === props.commits.length - 1 && !props.onLoadMore}
                onPress={() => props.openCommit(item)}
              />
            )}
            onEndReached={
              page !== 'commits' ||
              props.error ||
              props.loading ||
              props.loadingMore
                ? undefined
                : props.onLoadMore
            }
            onEndReachedThreshold={0.5}
            refreshing={props.loading && props.commits.length > 0}
            onRefresh={props.onRefresh}
            ListEmptyComponent={
              notice ?? <Text style={[s.notice, muted]}>No commits.</Text>
            }
            ListFooterComponent={
              props.loadingMore ? (
                <ActivityIndicator color={c.textSecondary} />
              ) : props.commits.length && props.error ? (
                (notice ?? undefined)
              ) : undefined
            }
          />
        </View>
      )}
      {page !== 'commits' && (
        <FlatList
          key={page}
          data={files ?? []}
          keyExtractor={(f) => f.path}
          contentContainerStyle={s.fileList}
          renderItem={({ item }) => <FileRow file={item} />}
          refreshing={props.loading && !!files}
          onRefresh={props.onRefresh}
          ListHeaderComponent={header}
          ListFooterComponent={notice ?? undefined}
        />
      )}
    </SafeAreaView>
  );
}

const s = StyleSheet.create({
  screen: { flex: 1 },
  flex: { flex: 1, minWidth: 0 },
  nav: {
    height: 52,
    paddingHorizontal: 12,
    justifyContent: 'center',
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  back: {
    width: 36,
    height: 36,
    borderRadius: 18,
    alignItems: 'center',
    justifyContent: 'center',
    zIndex: 1,
  },
  heading: { position: 'absolute', left: 55, right: 55, alignItems: 'center' },
  title: { fontSize: 16, fontWeight: '600' },
  subtitle: { fontSize: 12 },
  commit: {
    flexDirection: 'row',
    alignItems: 'stretch',
    paddingHorizontal: 16,
    minHeight: 56,
  },
  commitBody: {
    flex: 1,
    minWidth: 0,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    paddingVertical: 8,
    borderBottomWidth: 1,
  },
  timeline: { width: 22, alignItems: 'flex-start' },
  line: { flex: 1, width: 1, marginLeft: 5 },
  dot: { width: 10, height: 10, borderRadius: 5 },
  meta: { fontSize: 12, fontFamily: mono, marginTop: 2 },
  stats: { fontSize: 12, fontFamily: mono },
  fileList: { paddingHorizontal: 16, paddingBottom: 32 },
  file: {
    flexDirection: 'row',
    alignItems: 'center',
    minHeight: 52,
    borderBottomWidth: 1,
    gap: 12,
  },
  summaryRow: {
    minHeight: 44,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
  },
  subject: { fontSize: 18, fontWeight: '600', paddingVertical: 14 },
  body: { fontSize: 14, marginBottom: 12 },
  fact: {
    minHeight: 48,
    paddingHorizontal: 16,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderColor: 'transparent',
  },
  notice: { padding: 16, alignItems: 'center', gap: 8 },
});
