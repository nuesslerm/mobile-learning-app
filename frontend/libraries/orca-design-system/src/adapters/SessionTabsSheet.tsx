import type { SessionTabsSheetProps } from '@lib/agent-ui-contracts';
import { useEffect, useRef, useState } from 'react';
import { useReducedMotionEnabled } from './useReducedMotion';
import { Check, SquareTerminal } from 'lucide-react-native';
import {
  Animated,
  Easing,
  Modal,
  Platform,
  useWindowDimensions,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from 'react-native';
import { homePalettes, useOrcaScheme } from './appearance';

export function SessionTabsSheet(props: SessionTabsSheetProps) {
  const scheme = useOrcaScheme();
  const colors = homePalettes[scheme];
  const reduced = useReducedMotionEnabled();
  const progress = useRef(new Animated.Value(0)).current;
  const [settled, setSettled] = useState(false);
  const [sheetHeight, setSheetHeight] = useState(0);
  const { height } = useWindowDimensions();
  const closing = useRef(false);
  useEffect(() => {
    const animation = Animated.timing(progress, {
      toValue: 1,
      duration: reduced ? 200 : 480,
      easing: Easing.bezier(0.32, 0.72, 0, 1),
      useNativeDriver: Platform.OS !== 'web',
    });
    if (!closing.current)
      animation.start(({ finished }) => {
        if (finished) setSettled(true);
      });
    return () => animation.stop();
  }, [progress, reduced]);
  function dismiss(after: () => void) {
    if (closing.current) return;
    closing.current = true;
    setSettled(false);
    Animated.timing(progress, {
      toValue: 0,
      duration: reduced ? 150 : 300,
      easing: Easing.bezier(0.32, 0.72, 0, 1),
      useNativeDriver: Platform.OS !== 'web',
    }).start(({ finished }) => {
      if (finished) after();
    });
  }
  const title =
    props.sessions.length === 1 ? '1 tab' : props.sessions.length + ' tabs';
  return (
    <Modal
      visible={props.visible}
      transparent
      animationType="none"
      onRequestClose={() => dismiss(props.onClose)}
    >
      <View style={styles.overlay}>
        <Animated.View style={[StyleSheet.absoluteFill, { opacity: progress }]}>
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Dismiss"
            style={styles.backdrop}
            onPress={() => dismiss(props.onClose)}
          />
        </Animated.View>
        <Animated.View
          onLayout={(event) => setSheetHeight(event.nativeEvent.layout.height)}
          accessibilityElementsHidden={!settled}
          importantForAccessibility={settled ? 'auto' : 'no-hide-descendants'}
          accessibilityViewIsModal
          style={[
            styles.sheet,
            reduced
              ? { opacity: progress }
              : {
                  transform: [
                    {
                      translateY: progress.interpolate({
                        inputRange: [0, 1],
                        outputRange: [sheetHeight || height, 0],
                      }),
                    },
                  ],
                },
            {
              backgroundColor: scheme === 'light' ? '#f5f5f5' : colors.bgPanel,
              paddingBottom: Math.max(16, props.bottomInset) + 16,
            },
          ]}
        >
          <View
            style={[styles.handle, { backgroundColor: colors.textMuted }]}
          />
          <View style={styles.header}>
            <Text
              accessibilityRole="header"
              style={[styles.title, { color: colors.textPrimary }]}
            >
              {title}
            </Text>
            <Text
              numberOfLines={1}
              style={{ fontSize: 12, color: colors.textMuted }}
            >
              {props.workspace}
            </Text>
          </View>
          <ScrollView
            bounces={false}
            style={[styles.group, { backgroundColor: colors.bgGroup }]}
          >
            {props.sessions.map((tab, index) => (
              <View key={tab.id}>
                {index > 0 && (
                  <View
                    style={[
                      styles.separator,
                      { backgroundColor: colors.borderSubtle },
                    ]}
                  />
                )}
                <Pressable
                  accessibilityRole="button"
                  accessibilityState={{ selected: tab.id === props.activeId }}
                  accessibilityLabel={tab.title}
                  onPress={() => dismiss(() => props.onSelect(tab.id))}
                  style={styles.row}
                >
                  <View
                    style={[styles.icon, { backgroundColor: colors.bgRaised }]}
                  >
                    <SquareTerminal size={17} color={colors.textSecondary} />
                  </View>
                  <Text
                    numberOfLines={1}
                    style={[styles.name, { color: colors.textPrimary }]}
                  >
                    {tab.title}
                  </Text>
                  {tab.id === props.activeId && (
                    <Check size={17} color={colors.accentBlue} />
                  )}
                </Pressable>
              </View>
            ))}
          </ScrollView>
          {props.error && (
            <Pressable
              accessibilityRole="button"
              onPress={props.onRetry}
              style={styles.retry}
            >
              <Text style={{ color: colors.textSecondary }}>{props.error}</Text>
              <Text style={{ color: colors.accentBlue }}>Try again</Text>
            </Pressable>
          )}
        </Animated.View>
      </View>
    </Modal>
  );
}

const styles = StyleSheet.create({
  overlay: { flex: 1, justifyContent: 'flex-end' },
  backdrop: { ...StyleSheet.absoluteFill, backgroundColor: 'rgba(0,0,0,0.55)' },
  sheet: {
    maxHeight: '88%',
    borderTopLeftRadius: 24,
    borderTopRightRadius: 24,
    borderCurve: 'continuous',
    paddingHorizontal: 12,
    paddingTop: 8,
  },
  handle: {
    width: 36,
    height: 5,
    alignSelf: 'center',
    borderRadius: 99,
    opacity: 0.5,
    marginBottom: 12,
  },
  header: { height: 52, alignItems: 'center', justifyContent: 'center' },
  title: { fontSize: 16, fontWeight: '600' },
  group: { flexGrow: 0, borderRadius: 16, overflow: 'hidden' },
  row: {
    minHeight: 52,
    paddingHorizontal: 16,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
  },
  icon: {
    width: 34,
    height: 34,
    borderRadius: 10,
    alignItems: 'center',
    justifyContent: 'center',
  },
  name: { flex: 1, minWidth: 0, fontSize: 15, fontWeight: '600' },
  separator: { height: StyleSheet.hairlineWidth, marginLeft: 62 },
  retry: { padding: 12, alignItems: 'center' },
});
