import UnfoldVertical from 'lucide-react-native/dist/esm/icons/unfold-vertical.mjs';
import ChevronDown from 'lucide-react-native/dist/esm/icons/chevron-down.mjs';
import Check from 'lucide-react-native/dist/esm/icons/check.mjs';
import { memo, useMemo, useState } from 'react';
import {
  ActivityIndicator,
  FlatList,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
  useWindowDimensions,
} from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { fileReviewHeading, foldedRows, type SourceCodeLine, type SourceFileReview } from '@lib/skin';
import { BackIcon, ChevronRightIcon } from './homeIcons';
import { useChatPalette } from './chatPalette';
import { useOrcaScheme } from './appearance';
import { highlightCode } from './codeSyntax';

const CodeRow = memo(function CodeRow({
  line,
  extension,
  gutter,
}: {
  line: SourceCodeLine;
  extension: string;
  gutter: number;
}) {
  const c = useChatPalette();
  const dark = useOrcaScheme() === 'dark';
  const { segments } = useMemo(
    () => highlightCode(line.text, extension),
    [line.text, extension]
  );
  const added = line.kind === 'add',
    deleted = line.kind === 'delete';
  const backgroundColor = added
    ? dark
      ? '#173524'
      : '#dafbe1'
    : deleted
      ? dark
        ? '#3b2225'
        : '#ffebe9'
      : c.bgBase;
  const number = deleted ? line.oldLineNumber : line.newLineNumber;
  return (
    <View style={[styles.row, { backgroundColor }]}>
      <Text
        style={[
          styles.code,
          styles.number,
          {
            width: gutter,
            color: added || deleted ? c.textPrimary : c.textMuted,
            backgroundColor: added
              ? dark
                ? '#285731'
                : '#aceebb'
              : deleted
                ? dark
                  ? '#643139'
                  : '#ffcecb'
                : c.bgBase,
          },
        ]}
      >
        {number}
      </Text>
      <Text
        style={[
          styles.code,
          styles.sign,
          { color: added ? c.statusGreen : c.statusRed },
        ]}
      >
        {added ? '+' : deleted ? '−' : ' '}
      </Text>
      <Text style={[styles.code, { color: c.textPrimary }]}>
        {segments.map((part, i) => (
          <Text
            key={i}
            style={
              part.kind === 'plain' ? undefined : { color: c.syntax[part.kind] }
            }
          >
            {part.text}
          </Text>
        ))}
      </Text>
    </View>
  );
});

function FileLines({ lines, path }: { lines: SourceCodeLine[]; path: string }) {
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const rows = useMemo(() => foldedRows(lines, expanded), [lines, expanded]);
  const dark = useOrcaScheme() === 'dark';
  const width = useWindowDimensions().width,
    c = useChatPalette();
  const extension = path.split('.').pop()?.toLowerCase() ?? '';
  const digits = useMemo(
    () =>
      lines.reduce(
        (max, line) =>
          Math.max(
            max,
            String(line.newLineNumber ?? line.oldLineNumber ?? '').length
          ),
        1
      ),
    [lines]
  );
  const gutter = Math.ceil(12 + digits * 7.22);
  const contentWidth = useMemo(
    () =>
      lines.reduce(
        (max, line) => Math.max(max, line.text.replace(/\t/g, '    ').length),
        0
      ) *
        7.4 +
      gutter +
      28,
    [lines, gutter]
  );
  return (
    <ScrollView
      horizontal
      contentContainerStyle={{ minWidth: '100%' }}
      style={styles.fill}
    >
      <FlatList
        style={{ width: Math.max(width, contentWidth) }}
        data={rows}
        keyExtractor={(r) => String(r.index)}
        renderItem={({ item }) =>
          'fold' in item ? (
            <Pressable
              accessibilityRole="button"
              accessibilityLabel={'Expand ' + item.fold + ' unchanged lines'}
              onPress={() =>
                setExpanded((prev) => new Set(prev).add(item.index))
              }
              style={[
                styles.fold,
                { backgroundColor: dark ? '#18364a' : '#ddf4ff' },
              ]}
            >
              <View
                style={[
                  styles.foldMarker,
                  {
                    width: gutter + 14,
                    backgroundColor: dark
                      ? 'rgba(56,139,253,0.22)'
                      : 'rgba(84,174,255,0.4)',
                  },
                ]}
              >
                <UnfoldVertical
                  size={14}
                  color={dark ? '#4493f8' : '#0969da'}
                />
              </View>
              <Text
                style={{ color: dark ? '#4493f8' : '#0969da', fontSize: 12 }}
              >
                {item.fold} unchanged lines
              </Text>
              {item.scope && (
                <Text
                  numberOfLines={1}
                  style={[styles.code, { color: c.textMuted }]}
                >
                  {item.scope}
                </Text>
              )}
            </Pressable>
          ) : (
            <CodeRow line={item.line} gutter={gutter} extension={extension} />
          )
        }
      />
    </ScrollView>
  );
}

export function FileReview({ review: r }: { review: SourceFileReview }) {
  const c = useChatPalette(),
    insets = useSafeAreaInsets();
  const [picker, showPicker] = useState(false);
  const { file, title, subtitle, previous, next, advanceLabel, marksViewed } = fileReviewHeading(r);
  const text = { color: c.textPrimary },
    muted = { color: c.textMuted };
  const control = [styles.control, { backgroundColor: c.bgRaised }];
  return (
    <View style={[styles.fill, { backgroundColor: c.bgBase }]}>
      <View style={[styles.header, { borderColor: c.borderSubtle }]}>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Back to changed files"
          onPress={r.back}
          style={control}
        >
          <BackIcon size={20} color={c.textPrimary} />
        </Pressable>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Select changed file"
          onPress={() => showPicker(!picker)}
          style={styles.title}
        >
          <View style={styles.titleRow}>
            <Text numberOfLines={1} style={[text, styles.titleText]}>
              {title}
            </Text>
            <ChevronDown size={14} color={c.textMuted} />
          </View>
          <Text style={[muted, styles.small]}>{subtitle}</Text>
        </Pressable>
      </View>
      {picker ? (
        <FlatList
          data={r.files}
          keyExtractor={(item) => item.path}
          renderItem={({ item, index }) => (
            <Pressable
              accessibilityRole="button"
              style={styles.choice}
              onPress={() => {
                showPicker(false);
                r.select(index);
              }}
            >
              <Text style={text}>{item.path}</Text>
            </Pressable>
          )}
        />
      ) : (
        <>
          <View style={[styles.path, { borderColor: c.borderSubtle }]}>
            <Text
              style={[styles.pathText, styles.fill, muted]}
              numberOfLines={1}
              ellipsizeMode="middle"
            >
              {file.path}
            </Text>
            {!!file.additions && (
              <Text style={[styles.pathText, { color: c.statusGreen }]}>
                +{file.additions}
              </Text>
            )}
            {!!file.deletions && (
              <Text style={[styles.pathText, { color: c.statusRed }]}>
                −{file.deletions}
              </Text>
            )}
            {r.branch && (
              <Pressable
                accessibilityRole="checkbox"
                accessibilityLabel="Viewed"
                accessibilityState={{ checked: r.viewed.has(file.path) }}
                onPress={r.toggleViewed}
                style={[styles.check, { borderColor: c.textMuted }]}
              >
                <Text style={text}>{r.viewed.has(file.path) ? '✓' : ''}</Text>
              </Pressable>
            )}
          </View>
          {r.loading ? (
            <ActivityIndicator style={styles.notice} color={c.textMuted} />
          ) : r.message ? (
            <View style={styles.notice}>
              <Text style={muted}>{r.message}</Text>
              {r.retry && (
                <Pressable accessibilityRole="button" onPress={r.retry}>
                  <Text style={{ color: c.accentBlue }}>Retry</Text>
                </Pressable>
              )}
            </View>
          ) : r.lines?.length ? (
            <FileLines key={file.path} lines={r.lines} path={file.path} />
          ) : (
            <Text style={[muted, styles.notice]}>No text changes.</Text>
          )}
          <View
            style={[
              styles.footer,
              {
                paddingBottom: insets.bottom + 8,
                borderColor: c.borderSubtle,
                backgroundColor: c.bgBase,
              },
            ]}
          >
            <Pressable
              accessibilityRole="button"
              accessibilityLabel="Previous file"
              onPress={previous}
              style={[control, styles.footerControl]}
            >
              <BackIcon size={18} color={c.textPrimary} />
            </Pressable>
            <Pressable
              accessibilityRole="button"
              onPress={r.advance}
              style={[styles.advance, { backgroundColor: c.textPrimary }]}
            >
              {marksViewed ? (
                <Check size={16} color={c.bgBase} />
              ) : (
                <ChevronRightIcon size={16} color={c.bgBase} />
              )}
              <Text style={{ fontWeight: '600', color: c.bgBase }}>
                {advanceLabel}
              </Text>
            </Pressable>
            <Pressable
              accessibilityRole="button"
              accessibilityLabel="Next changed file"
              onPress={next}
              style={[control, styles.footerControl]}
            >
              <ChevronRightIcon size={18} color={c.textPrimary} />
            </Pressable>
          </View>
        </>
      )}
    </View>
  );
}
const styles = StyleSheet.create({
  fill: { flex: 1 },
  header: {
    height: 52,
    flexDirection: 'row',
    alignItems: 'center',
    paddingHorizontal: 12,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  control: {
    width: 36,
    height: 36,
    borderRadius: 18,
    alignItems: 'center',
    justifyContent: 'center',
  },
  title: { position: 'absolute', left: 62, right: 62, alignItems: 'center' },
  titleText: { fontSize: 16, fontWeight: '600' },
  small: { fontSize: 12 },
  path: {
    paddingHorizontal: 16,
    paddingVertical: 8,
    gap: 8,
    flexDirection: 'row',
    alignItems: 'center',
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  check: {
    height: 20,
    width: 20,
    borderWidth: 1,
    borderRadius: 4,
    alignItems: 'center',
    justifyContent: 'center',
  },
  pathText: {
    fontFamily: Platform.OS === 'ios' ? 'Menlo' : 'monospace',
    fontSize: 12,
  },
  code: {
    fontFamily: Platform.OS === 'ios' ? 'Menlo' : 'monospace',
    fontSize: 12,
    lineHeight: 18,
  },
  number: { textAlign: 'right', paddingRight: 4 },
  sign: { width: 14, textAlign: 'center' },
  row: { flexDirection: 'row', alignItems: 'flex-start' },
  fold: { height: 32, alignItems: 'center', gap: 8, flexDirection: 'row' },
  foldMarker: {
    alignSelf: 'stretch',
    alignItems: 'center',
    justifyContent: 'center',
  },
  footerControl: { width: 48, height: 48 },
  footer: {
    borderTopWidth: StyleSheet.hairlineWidth,
    paddingTop: 8,
    paddingHorizontal: 16,
    flexDirection: 'row',
    gap: 8,
    alignItems: 'center',
  },
  titleRow: {
    maxWidth: '100%',
    alignItems: 'center',
    gap: 4,
    flexDirection: 'row',
  },
  advance: {
    flexDirection: 'row',
    gap: 6,
    flex: 1,
    minHeight: 48,
    justifyContent: 'center',
    alignItems: 'center',
    borderRadius: 12,
  },
  choice: { padding: 14 },
  notice: { flex: 1, padding: 16, alignItems: 'center', gap: 8 },
});
