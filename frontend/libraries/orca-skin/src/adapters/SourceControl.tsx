import {
  commitAge,
  fileStatusLetter,
  fileTotals,
  sourceControlHeading,
  type SourceControlProps,
  type SourceCommit,
  type SourceFile,
} from '@lib/skin';
import GitBranch from 'lucide-react-native/dist/esm/icons/git-branch.mjs';
import ChevronDown from 'lucide-react-native/dist/esm/icons/chevron-down.mjs';
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
import { PullRequestPanel, SourceControlTabs } from './SourceControlTabs';
import { BranchSheet } from './BranchSheet';
import { FileReview } from './FileReview';
import { useChatPalette } from './chatPalette';

const mono = Platform.OS === 'ios' ? 'Menlo' : 'monospace';

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
            {[commit.shortSha, commit.author, commitAge(commit.authoredAtMs)]
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

function FileRow({ file, onPress }: { file: SourceFile; onPress: () => void }) {
  const c = useChatPalette();
  const pos = file.path.lastIndexOf('/');
  const status = fileStatusLetter(file);
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={file.path}
      onPress={onPress}
      style={[s.file, { borderColor: c.borderSubtle }]}
    >
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
    </Pressable>
  );
}

export function SourceControl(props: SourceControlProps) {
  const c = useChatPalette();
  const { page, selected, detail, comparison } = props;
  const { title, subtitle, backLabel, branch } = sourceControlHeading(props);
  const text = { color: c.textPrimary };
  const muted = { color: c.textSecondary };
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
  const total = fileTotals(detail?.files);
  const ahead = comparison?.ahead;
  const header =
    page === 'changes' ? (
      <>
        <View style={s.branchRow}>
          <Pressable accessibilityRole="button" accessibilityLabel="Switch Branch" disabled={props.switchingBranch} onPress={props.openBranches} style={s.branchButton}>
            <GitBranch size={15} strokeWidth={2.1} color={c.textSecondary} />
            <Text style={[s.branchLabel, text]} numberOfLines={1}>{branch ?? 'No branch'}</Text>
            <ChevronDown size={14} strokeWidth={2.2} color={c.textMuted} />
          </Pressable>
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
      <View style={props.review ? { display: 'none' } : s.flex}>
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
        {page === 'changes' && <SourceControlTabs selected={props.tab} select={props.selectTab} />}
        {page === 'changes' && props.tab === 'pr' && <PullRequestPanel panel={props.prPanel} />}
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
        {page !== 'commits' && (page !== 'changes' || props.tab === 'changes') && (
          <FlatList
            key={page}
            data={files ?? []}
            keyExtractor={(f) => f.path}
            contentContainerStyle={s.fileList}
            renderItem={({ item, index }) => (
              <FileRow file={item} onPress={() => props.openFile(index)} />
            )}
            refreshing={props.loading && !!files}
            onRefresh={props.onRefresh}
            ListHeaderComponent={header}
            ListFooterComponent={notice ?? undefined}
          />
        )}
      </View>
      {props.review && <FileReview review={props.review} />}
      {props.picker && <BranchSheet {...props.picker} />}
    </SafeAreaView>
  );
}

const s = StyleSheet.create({
  screen: { flex: 1 },
  branchRow: { paddingHorizontal: 16, paddingTop: 12, paddingBottom: 4, alignItems: 'flex-start' },
  branchButton: { minHeight: 32, flexDirection: 'row', alignItems: 'center', gap: 6, maxWidth: '100%' },
  branchLabel: { flexShrink: 1, fontSize: 15, fontWeight: '600' },
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
