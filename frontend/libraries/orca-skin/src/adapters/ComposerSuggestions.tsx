import { BottomSheet } from './BottomSheet';
import {
  useComposerSuggestionRows,
  type ComposerAddButtonProps,
  type ComposerSuggestionListProps,
} from '@lib/skin';
import { AtSign, FileText, Package, Plus, Terminal } from 'lucide-react-native';
import { Fragment, useState } from 'react';
import {
  Keyboard,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { useChatPalette } from './chatPalette';

const monoFamily = Platform.select({
  ios: 'Menlo',
  web: 'ui-monospace, "SF Mono", SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace',
  default: 'monospace',
});

const choices = [
  { kind: 'file', label: 'Workspace file', Icon: AtSign },
  { kind: 'skill', label: 'Skills', Icon: Package },
  { kind: 'command', label: 'Commands', Icon: Terminal },
] as const;
const icons = { file: FileText, skill: Package, command: Terminal };

export function ComposerAddButton({
  onSelect,
  onDismiss,
}: ComposerAddButtonProps) {
  const [open, setOpen] = useState(false);
  const colors = useChatPalette();
  const { bottom } = useSafeAreaInsets();
  return (
    <>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel="Add to message"
        onPress={() => {
          Keyboard.dismiss();
          setOpen(true);
        }}
        style={[styles.plus, { backgroundColor: colors.bgRaised }]}
      >
        <Plus size={19} color={colors.textPrimary} />
      </Pressable>
      <BottomSheet
        visible={open}
        onClose={() => setOpen(false)}
        onDismiss={onDismiss}
        style={[
          styles.sheet,
          { backgroundColor: colors.sheetSurface, paddingBottom: bottom + 16 },
        ]}
      >
        {(dismiss) => (
          <>
            <View
              style={[styles.handle, { backgroundColor: colors.textMuted }]}
            />
            <View style={[styles.group, { backgroundColor: colors.bgGroup }]}>
              {choices.map(({ kind, label, Icon }, index) => (
                <Fragment key={kind}>
                  {index > 0 && (
                    <View
                      style={[
                        styles.rule,
                        { backgroundColor: colors.borderSubtle },
                      ]}
                    />
                  )}
                  <Pressable
                    accessibilityRole="button"
                    accessibilityLabel={label}
                    onPress={() =>
                      dismiss(() => {
                        onSelect(kind);
                        setOpen(false);
                      })
                    }
                    style={styles.row}
                  >
                    <View style={styles.icon}>
                      <Icon
                        size={16}
                        strokeWidth={1.8}
                        color={colors.textSecondary}
                      />
                    </View>
                    <Text style={{ fontSize: 15, color: colors.textPrimary }}>
                      {label}
                    </Text>
                  </Pressable>
                </Fragment>
              ))}
            </View>
          </>
        )}
      </BottomSheet>
    </>
  );
}

export function ComposerSuggestionList({
  source,
  sigil,
  query,
  kind,
  onPick,
}: ComposerSuggestionListProps) {
  const colors = useChatPalette();
  const { error, empty, retry, groups } = useComposerSuggestionRows(source, sigil, query, kind);
  if (empty) return null;
  return (
    <View
      style={[
        styles.card,
        { backgroundColor: colors.bgPanel, borderColor: colors.composerBorder },
      ]}
    >
      <ScrollView style={{ maxHeight: 300 }} keyboardShouldPersistTaps="always">
        {error && (
          <Pressable
            onPress={retry}
            accessibilityRole="button"
            style={styles.suggestion}
          >
            <Text style={{ color: colors.textPrimary }}>
              Couldn’t load suggestions. Try again
            </Text>
          </Pressable>
        )}
        {groups.map(({ group, heading, rows }) => {
          const Icon = icons[group];
          return (
            <Fragment key={group}>
              <Text style={[styles.heading, { color: colors.textMuted }]}>
                {heading}
              </Text>
              {rows.map((row) => (
                <Pressable
                  key={row.token}
                  accessibilityRole="button"
                  accessibilityLabel={row.token}
                  onPress={() => onPick(row)}
                  style={({ pressed }) => [
                    styles.suggestion,
                    pressed && { backgroundColor: colors.bgRaised },
                  ]}
                >
                  <Icon
                    size={16}
                    color={colors.textSecondary}
                    style={styles.suggestionIcon}
                  />
                  <View style={styles.body}>
                    <View style={styles.nameLine}>
                      <Text
                        style={[styles.name, { color: colors.textPrimary }]}
                        numberOfLines={1}
                      >
                        {row.token}
                      </Text>
                      {!!row.hint && (
                        <Text
                          style={[styles.hint, { color: colors.textMuted }]}
                          numberOfLines={1}
                        >
                          {row.hint}
                        </Text>
                      )}
                    </View>
                    {!!row.description && (
                      <Text
                        numberOfLines={2}
                        style={[
                          styles.description,
                          { color: colors.textMuted },
                        ]}
                      >
                        {row.description}
                      </Text>
                    )}
                  </View>
                </Pressable>
              ))}
            </Fragment>
          );
        })}
      </ScrollView>
    </View>
  );
}

const styles = StyleSheet.create({
  plus: {
    width: 38,
    height: 38,
    borderRadius: 19,
    alignItems: 'center',
    justifyContent: 'center',
  },
  sheet: {
    borderTopLeftRadius: 24,
    borderTopRightRadius: 24,
    paddingHorizontal: 12,
  },
  handle: {
    width: 36,
    height: 5,
    borderRadius: 3,
    marginTop: 8,
    marginBottom: 12,
    alignSelf: 'center',
    opacity: 0.5,
  },
  group: { borderRadius: 16, overflow: 'hidden' },
  rule: { height: StyleSheet.hairlineWidth, marginLeft: 52 },
  row: {
    minHeight: 48,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
    paddingHorizontal: 16,
  },
  icon: { width: 24, alignItems: 'center' },
  card: {
    flexShrink: 1,
    marginBottom: 8,
    borderRadius: 24,
    borderWidth: StyleSheet.hairlineWidth,
    marginHorizontal: 10,
    paddingVertical: 6,
    overflow: 'hidden',
  },
  body: { flex: 1, minWidth: 0, gap: 2 },
  nameLine: { flexDirection: 'row', alignItems: 'baseline', gap: 6 },
  name: { fontFamily: monoFamily, fontSize: 13.5, flexShrink: 1 },
  hint: { fontFamily: monoFamily, fontSize: 12, flexShrink: 1 },
  suggestionIcon: { marginTop: 2 },
  description: { fontSize: 12.5, lineHeight: 17 },
  heading: {
    fontSize: 11,
    fontWeight: '600',
    letterSpacing: 0.5,
    textTransform: 'uppercase',
    paddingHorizontal: 16,
    paddingTop: 8,
    paddingBottom: 4,
  },
  suggestion: {
    flexDirection: 'row',
    gap: 10,
    paddingHorizontal: 16,
    paddingVertical: 8,
    minHeight: 44,
  },
});
