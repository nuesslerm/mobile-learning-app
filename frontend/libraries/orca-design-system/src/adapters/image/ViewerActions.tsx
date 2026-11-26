import type { ChatImageViewerProps } from '@lib/agent-ui-contracts';
import { Copy, Download, Share } from 'lucide-react-native';
import { Fragment, useEffect, useRef, useState } from 'react';
import { Pressable, StyleSheet, Text, View } from 'react-native';
import { useChatPalette } from '../chatPalette';

const rows = [
  {
    id: 'copy',
    label: 'Copy image',
    Icon: Copy,
    success: 'Image copied',
    failure: "Couldn't copy image",
  },
  {
    id: 'save',
    label: 'Save image',
    Icon: Download,
    success: 'Image saved',
    failure: "Couldn't save image",
  },
  {
    id: 'share',
    label: 'Share…',
    Icon: Share,
    success: '',
    failure: "Couldn't share image",
  },
] as const;

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
  const running = useRef(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('');
  useEffect(() => {
    if (!message) return;
    const timer = setTimeout(() => setMessage(''), 2400);
    return () => clearTimeout(timer);
  }, [message]);
  if (!actions || !rows.some((row) => actions[row.id])) return null;

  async function run(row: (typeof rows)[number]) {
    const action = actions?.[row.id];
    if (running.current || !action) return;
    running.current = true;
    setBusy(true);
    setOpen(false);
    setMessage('');
    try {
      await action();
      setMessage(row.success);
    } catch {
      setMessage(row.failure);
    } finally {
      running.current = false;
      setBusy(false);
    }
  }
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
        <View accessibilityViewIsModal style={styles.overlay}>
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Dismiss image actions"
            style={StyleSheet.absoluteFill}
            onPress={() => setOpen(false)}
          />
          <View
            style={[
              styles.sheet,
              {
                paddingBottom: bottomInset + 16,
                backgroundColor: colors.sheetSurface,
              },
            ]}
          >
            <View
              style={[styles.handle, { backgroundColor: colors.textMuted }]}
            />
            <View style={[styles.group, { backgroundColor: colors.bgGroup }]}>
              {rows
                .filter((row) => actions[row.id])
                .map((row, index) => (
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
                      onPress={() => void run(row)}
                      style={styles.row}
                    >
                      <View style={styles.leading}>
                        <row.Icon
                          size={16}
                          strokeWidth={1.8}
                          color={colors.textSecondary}
                        />
                      </View>
                      <Text style={{ fontSize: 15, color: colors.textPrimary }}>
                        {row.label}
                      </Text>
                    </Pressable>
                  </Fragment>
                ))}
            </View>
          </View>
        </View>
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
  overlay: {
    ...StyleSheet.absoluteFill,
    justifyContent: 'flex-end',
    backgroundColor: 'rgba(0,0,0,0.55)',
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
