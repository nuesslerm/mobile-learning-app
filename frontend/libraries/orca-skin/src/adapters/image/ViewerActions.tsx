import { BottomSheet } from '../BottomSheet';
import { useImageActions, type ChatImageViewerProps } from '@lib/skin';
import { Copy, Download, Share } from 'lucide-react-native';
import { Fragment } from 'react';
import { Pressable, StyleSheet, Text, View } from 'react-native';
import { useChatPalette } from '../chatPalette';

const icons = { copy: Copy, save: Download, share: Share } as const;

function ActionIcon({ id, color }: { id: keyof typeof icons; color: string }) {
  const Icon = icons[id];
  return <Icon size={16} strokeWidth={1.8} color={color} />;
}

export function ViewerActions({
  actions,
  topInset,
  rightInset,
  bottomInset = 0,
  open,
  setOpen,
}: Pick<
  ChatImageViewerProps,
  'actions' | 'topInset' | 'rightInset' | 'bottomInset'
> & {
  open: boolean;
  setOpen: (open: boolean) => void;
}) {
  const colors = useChatPalette();
  const { busy, message, offered, run } = useImageActions(actions, setOpen);
  if (!offered.length) return null;
  return (
    <>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel="Image actions"
        accessibilityElementsHidden={open}
        importantForAccessibility={open ? 'no-hide-descendants' : 'auto'}
        disabled={busy || open}
        onPress={() => setOpen(true)}
        style={[
          styles.toggle,
          {
            top: topInset + 8,
            right: rightInset + 68,
            backgroundColor: colors.bgRaised,
          },
        ]}
      >
        <Share size={20} strokeWidth={2} color={colors.textPrimary} />
      </Pressable>
      {!!message && (
        <View
          pointerEvents="none"
          style={[
            styles.toast,
            { bottom: bottomInset + 20, backgroundColor: colors.textPrimary },
          ]}
        >
          <Text accessibilityRole="alert" style={{ color: colors.bgBase }}>
            {message}
          </Text>
        </View>
      )}
      {open && (
        <BottomSheet
          label="Dismiss image actions"
          onClose={() => setOpen(false)}
          style={[
            styles.sheet,
            {
              paddingBottom: bottomInset + 16,
              backgroundColor: colors.sheetSurface,
            },
          ]}
        >
          {(dismiss) => (
            <>
              <View
                style={[styles.handle, { backgroundColor: colors.textMuted }]}
              />
              <View style={[styles.group, { backgroundColor: colors.bgGroup }]}>
                {offered.map((row, index) => (
                    <Fragment key={row.id}>
                      {index > 0 && (
                        <View
                          style={{
                            height: StyleSheet.hairlineWidth,
                            marginLeft: 52,
                            backgroundColor: colors.borderSubtle,
                          }}
                        />
                      )}
                      <Pressable
                        accessibilityRole="button"
                        onPress={() => dismiss(() => run(row))}
                        style={styles.row}
                      >
                        <View style={styles.leading}>
                          <ActionIcon id={row.id} color={colors.textSecondary} />
                        </View>
                        <Text
                          style={{ fontSize: 15, color: colors.textPrimary }}
                        >
                          {row.label}
                        </Text>
                      </Pressable>
                    </Fragment>
                  ))}
              </View>
            </>
          )}
        </BottomSheet>
      )}
    </>
  );
}

const styles = StyleSheet.create({
  toggle: {
    position: 'absolute',
    width: 44,
    height: 44,
    borderRadius: 999,
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
    opacity: 0.5,
    alignSelf: 'center',
    marginTop: 8,
    marginBottom: 12,
  },
  group: { borderRadius: 16, overflow: 'hidden' },
  row: {
    minHeight: 48,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
    paddingHorizontal: 16,
  },
  leading: { width: 24, alignItems: 'center' },
  toast: {
    position: 'absolute',
    alignSelf: 'center',
    borderRadius: 12,
    paddingVertical: 8,
    paddingHorizontal: 14,
  },
});
