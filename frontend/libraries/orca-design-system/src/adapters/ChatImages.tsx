import { ViewerActions } from './image/ViewerActions';
import type {
  ChatImageProps,
  ChatImageViewerProps,
} from '@lib/agent-ui-contracts';
import { X } from 'lucide-react-native';
import { useEffect, useRef, useState, type ReactNode } from 'react';
import {
  Animated,
  Easing,
  Image,
  Modal,
  Platform,
  Pressable,
  StyleSheet,
  Text,
  View,
} from 'react-native';
import { useChatPalette } from './chatPalette';
import { PanZoomImage } from './image/PanZoomImage';
import { useReducedMotionEnabled } from './useReducedMotion';

export function ChatSurface({ children }: { children: ReactNode }) {
  const colors = useChatPalette();
  return (
    <View style={{ flex: 1, backgroundColor: colors.bgBase }}>{children}</View>
  );
}
export function ChatImage({ uri, label, onOpen, unavailable }: ChatImageProps) {
  const colors = useChatPalette();
  const style = [
    styles.thumb,
    { backgroundColor: colors.bgRaised, borderColor: colors.borderSubtle },
  ];
  if (unavailable)
    return <Text style={{ color: colors.textSecondary }}>🖼 {label}</Text>;
  if (!uri) return <View style={style} accessibilityLabel={label} />;
  return (
    <Pressable
      accessibilityRole="imagebutton"
      accessibilityLabel={label}
      accessibilityHint="Opens the image full size"
      style={({ pressed }) => pressed && { opacity: 0.6 }}
      onPress={onOpen}
    >
      <Image source={{ uri }} resizeMode="contain" style={style} />
    </Pressable>
  );
}
export function ChatImageViewer({
  uri,
  label,
  topInset,
  rightInset,
  bottomInset,
  actions,
  onClose,
}: ChatImageViewerProps) {
  const colors = useChatPalette();
  const reduced = useReducedMotionEnabled() === true;
  const shown = useRef(new Animated.Value(0)).current;
  const closing = useRef(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const easing = Easing.bezier(0.32, 0.72, 0, 1);
  useEffect(() => {
    if (closing.current) return;
    const transition = Animated.timing(shown, {
      toValue: 1,
      duration: reduced ? 200 : 250,
      easing,
      useNativeDriver: Platform.OS !== 'web',
    });
    transition.start();
    return () => transition.stop();
  }, [shown, reduced]);
  function close() {
    if (closing.current) return;
    closing.current = true;
    Animated.timing(shown, {
      toValue: 0,
      duration: 200,
      easing,
      useNativeDriver: Platform.OS !== 'web',
    }).start(({ finished }) => {
      if (finished) onClose();
    });
  }
  return (
    <Modal visible transparent animationType="none" onRequestClose={() => menuOpen ? setMenuOpen(false) : close()}>
      <Animated.View
        style={{ flex: 1, backgroundColor: colors.bgBase, opacity: shown }}
      >
        <Animated.View
          style={{
            flex: 1,
            transform: [
              {
                scale: reduced
                  ? 1
                  : shown.interpolate({
                      inputRange: [0, 1],
                      outputRange: [0.95, 1],
                    }),
              },
            ],
          }}
        >
          <PanZoomImage uri={uri} label={label} />
        </Animated.View>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Close image"
          accessibilityElementsHidden={menuOpen}
          importantForAccessibility={menuOpen ? 'no-hide-descendants' : 'auto'}
          onPress={close}
          style={({ pressed }) => [
            styles.close,
            {
              top: topInset + 8,
              right: rightInset + 16,
              backgroundColor: colors.bgRaised,
              opacity: pressed ? 0.6 : 1,
            },
          ]}
        >
          <X size={22} color={colors.textPrimary} strokeWidth={2} />
        </Pressable>
        <ViewerActions actions={actions} topInset={topInset} rightInset={rightInset} bottomInset={bottomInset} open={menuOpen} setOpen={setMenuOpen} />
      </Animated.View>
    </Modal>
  );
}
const styles = StyleSheet.create({
  thumb: {
    width: 200,
    height: 150,
    borderRadius: 16,
    borderWidth: StyleSheet.hairlineWidth,
  },
  close: {
    position: 'absolute',
    width: 44,
    height: 44,
    alignItems: 'center',
    justifyContent: 'center',
    borderRadius: 999,
  },
});
