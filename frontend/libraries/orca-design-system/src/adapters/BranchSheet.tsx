import { BottomSheet } from './BottomSheet';
import type { SourceControlProps } from '@lib/agent-ui-contracts';
import { Fragment } from 'react';
import Check from 'lucide-react-native/dist/esm/icons/check.mjs';
import {
  ActivityIndicator,
  PanResponder,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  useWindowDimensions,
  View,
} from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { useChatPalette } from './chatPalette';

export function BranchSheet({
  branches,
  current,
  truncated,
  error,
  close,
  retry,
  select,
}: NonNullable<SourceControlProps['picker']>) {
  const colors = useChatPalette();
  const insets = useSafeAreaInsets();
  const { height } = useWindowDimensions();
  const handle = (dismiss: () => void) =>
    PanResponder.create({
      onMoveShouldSetPanResponder: (_, gesture) => gesture.dy > 6,
      onPanResponderRelease: (_, gesture) => {
        if (gesture.dy > 40 || gesture.vy > 0.6) dismiss();
      },
    });
  return (
    <BottomSheet
      label="Close branch picker"
      onClose={close}
      style={[
        s.sheet,
        {
          backgroundColor: colors.sheetSurface,
          maxHeight: height - insets.top - 16,
          paddingBottom: insets.bottom + 16,
        },
      ]}
    >
      {(dismiss) => (
        <>
          <View style={s.handleArea} {...handle(() => dismiss()).panHandlers}>
            <View style={[s.handle, { backgroundColor: colors.textMuted }]} />
          </View>
          <Text style={[s.title, { color: colors.textSecondary }]}>
            Switch Branch
          </Text>
          {branches ? (
            <ScrollView style={s.scroll} keyboardShouldPersistTaps="handled">
              <View style={[s.list, { backgroundColor: colors.bgGroup }]}>
                {branches.map((name, index) => (
                  <Fragment key={name}>
                    {index > 0 && (
                      <View
                        style={[
                          s.separator,
                          { backgroundColor: colors.borderSubtle },
                        ]}
                      />
                    )}
                    <Pressable
                      accessibilityRole="button"
                      accessibilityLabel={name}
                      accessibilityState={{ selected: current === name }}
                      onPress={() => dismiss(() => select(name))}
                      style={[s.row, current === name && s.currentRow]}
                    >
                      <View style={s.label}>
                        <Text
                          numberOfLines={1}
                          style={{ fontSize: 15, color: colors.textPrimary }}
                        >
                          {name}
                        </Text>
                        {name === current && (
                          <Text
                            style={{
                              fontSize: 13,
                              color: colors.textSecondary,
                            }}
                          >
                            current
                          </Text>
                        )}
                      </View>
                      {name === current && (
                        <Check
                          size={17}
                          strokeWidth={2.4}
                          color={colors.textPrimary}
                        />
                      )}
                    </Pressable>
                  </Fragment>
                ))}
              </View>
              {truncated && (
                <Text style={[s.note, { color: colors.textMuted }]}>
                  Branch list truncated.
                </Text>
              )}
            </ScrollView>
          ) : error ? (
            <Pressable
              accessibilityRole="button"
              onPress={retry}
              style={s.note}
            >
              <Text style={{ color: colors.textPrimary }}>{error} Retry</Text>
            </Pressable>
          ) : (
            <ActivityIndicator style={s.note} color={colors.textMuted} />
          )}
        </>
      )}
    </BottomSheet>
  );
}

const s = StyleSheet.create({
  sheet: {
    width: '100%',
    maxWidth: 640,
    alignSelf: 'center',
    borderTopLeftRadius: 24,
    borderTopRightRadius: 24,
    paddingHorizontal: 12,
  },
  handleArea: { alignItems: 'center', paddingTop: 8, paddingBottom: 12 },
  handle: { width: 36, height: 5, opacity: 0.5, borderRadius: 3 },
  title: {
    paddingHorizontal: 16,
    paddingBottom: 8,
    fontSize: 13,
    lineHeight: 17,
    fontWeight: '500',
  },
  scroll: { flexShrink: 1, flexGrow: 0 },
  list: { borderRadius: 16, overflow: 'hidden' },
  row: {
    minHeight: 48,
    paddingHorizontal: 16,
    paddingVertical: 8,
    alignItems: 'center',
    flexDirection: 'row',
    gap: 12,
  },
  separator: {
    height: StyleSheet.hairlineWidth,
    marginLeft: 16,
    transform: [{ translateY: -StyleSheet.hairlineWidth }],
  },
  currentRow: { minHeight: 60 },
  label: { flex: 1, gap: 2 },
  note: { padding: 16 },
});
