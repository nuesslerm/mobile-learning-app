import { useEffect, useRef } from 'react';
import { Animated, Easing, Pressable, StyleSheet } from 'react-native';
import { useChatPalette } from './chatPalette';
import { useOrcaScheme } from './appearance';
import { useReducedMotionEnabled } from './useReducedMotion';

export function WorkspaceSwitch({
  label,
  checked,
  disabled,
  onPress,
}: {
  label: string;
  checked: boolean;
  disabled: boolean;
  onPress: () => void;
}) {
  const colors = useChatPalette();
  const scheme = useOrcaScheme();
  const reduced = useReducedMotionEnabled();
  const progress = useRef(new Animated.Value(checked ? 1 : 0)).current;
  useEffect(() => {
    const motion = Animated.timing(progress, {
      toValue: checked ? 1 : 0,
      duration: reduced ? 0 : 260,
      easing: Easing.bezier(0.2, 0.8, 0.2, 1),
      useNativeDriver: false,
    });
    motion.start();
    return () => motion.stop();
  }, [checked, progress, reduced]);
  return (
    <Pressable
      accessibilityRole="switch"
      accessibilityLabel={label}
      accessibilityState={{ checked, disabled }}
      aria-checked={checked}
      disabled={disabled}
      onPress={onPress}
      hitSlop={{ top: 7, bottom: 7 }}
    >
      <Animated.View
        style={[
          styles.track,
          {
            backgroundColor: progress.interpolate({
              inputRange: [0, 1],
              outputRange: [
                scheme === 'light'
                  ? 'rgba(120,120,128,0.16)'
                  : 'rgba(120,120,128,0.36)',
                colors.accentBlue,
              ],
            }),
          },
        ]}
      >
        <Animated.View
          style={[
            styles.thumb,
            {
              transform: [
                {
                  translateX: progress.interpolate({
                    inputRange: [0, 1],
                    outputRange: [0, 20],
                  }),
                },
              ],
            },
          ]}
        />
      </Animated.View>
    </Pressable>
  );
}
const styles = StyleSheet.create({
  track: { width: 51, height: 31, borderRadius: 16, padding: 2 },
  thumb: {
    height: 27,
    width: 27,
    borderRadius: 14,
    backgroundColor: '#fff',
    shadowColor: '#000',
    shadowOpacity: 0.3,
    shadowOffset: { width: 0, height: 2 },
    shadowRadius: 3,
    elevation: 2,
  },
});
