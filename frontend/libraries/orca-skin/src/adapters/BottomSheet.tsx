import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import {
  Animated,
  Easing,
  KeyboardAvoidingView,
  Modal,
  Platform,
  Pressable,
  StyleSheet,
} from 'react-native';
import type { ViewProps, StyleProp, ViewStyle, View } from 'react-native';
import type { ComponentRef, Ref } from 'react';
import { useReducedMotionEnabled } from './useReducedMotion';

type Dismiss = (after?: () => void) => void;
type Props = {
  visible?: boolean;
  children: (dismiss: Dismiss) => ReactNode;
  onClose: () => void;
  onBack?: () => void;
  onDismiss?: () => void;
  label?: string;
  style: StyleProp<ViewStyle>;
  onLayout?: ViewProps['onLayout'];
  panelRef?: Ref<ComponentRef<typeof View>>;
  avoidKeyboard?: boolean;
  keyboardInset?: number;
};

export function BottomSheet({ visible = true, onDismiss, ...props }: Props) {
  const close = useRef<() => void>(props.onClose);
  return (
    <Modal
      visible={visible}
      transparent
      animationType="none"
      onDismiss={onDismiss}
      onRequestClose={() => close.current()}
    >
      {visible && <SheetSurface {...props} backHandler={close} />}
    </Modal>
  );
}

function SheetSurface({
  children,
  onClose,
  onBack,
  label = 'Dismiss',
  style,
  onLayout,
  panelRef,
  avoidKeyboard,
  keyboardInset,
  backHandler,
}: Props & { backHandler: { current: () => void } }) {
  const reduced = useReducedMotionEnabled();
  const [progress] = useState(() => new Animated.Value(0));
  const [height, setHeight] = useState(0);
  const closing = useRef(false);
  const pressed = useRef(false);
  const measured = height > 0;
  const offset = useMemo(
    () =>
      progress.interpolate({ inputRange: [0, 1], outputRange: [height, 0] }),
    [height, progress]
  );
  useEffect(() => {
    if (!measured || closing.current) return;
    const animation = Animated.timing(progress, {
      toValue: 1,
      duration: reduced ? 160 : 320,
      easing: Easing.bezier(0.32, 0.72, 0, 1),
      useNativeDriver: Platform.OS !== 'web',
    });
    animation.start();
    return () => animation.stop();
  }, [measured, progress, reduced]);

  function dismiss(after = onClose) {
    if (closing.current) return;
    closing.current = true;
    Animated.timing(progress, {
      toValue: 0,
      duration: reduced ? 120 : 200,
      easing: Easing.out(Easing.cubic),
      useNativeDriver: Platform.OS !== 'web',
    }).start(({ finished }) => {
      if (finished) after();
    });
  }

  useEffect(() => {
    backHandler.current = onBack ?? (() => dismiss());
  });
  return (
    <KeyboardAvoidingView
      style={[
        styles.overlay,
        keyboardInset ? { paddingBottom: keyboardInset } : undefined,
      ]}
      enabled={!!avoidKeyboard}
      behavior={avoidKeyboard && Platform.OS === 'ios' ? 'padding' : undefined}
    >
      <Animated.View
        pointerEvents="none"
        style={[styles.backdrop, { opacity: progress }]}
      />
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={label}
        style={StyleSheet.absoluteFill}
        onPointerDown={() => {
          pressed.current = true;
        }}
        onPointerCancel={() => {
          pressed.current = false;
        }}
        onPress={(event) => {
          // A long press can release over the newly mounted backdrop on web.
          if (
            Platform.OS !== 'web' ||
            pressed.current ||
            ('detail' in event.nativeEvent && event.nativeEvent.detail === 0)
          )
            dismiss();
          pressed.current = false;
        }}
      />
      <Animated.View
        ref={panelRef}
        accessibilityViewIsModal
        onLayout={(event) => {
          setHeight(event.nativeEvent.layout.height);
          onLayout?.(event);
        }}
        style={[
          style,
          {
            opacity: reduced ? progress : measured ? 1 : 0,
            transform: [{ translateY: reduced ? 0 : offset }],
          },
        ]}
      >
        {children(dismiss)}
      </Animated.View>
    </KeyboardAvoidingView>
  );
}

const styles = StyleSheet.create({
  overlay: { flex: 1, justifyContent: 'flex-end' },
  backdrop: { ...StyleSheet.absoluteFill, backgroundColor: 'rgba(0,0,0,0.55)' },
});
