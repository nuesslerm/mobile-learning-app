import type { SessionHeaderProps } from '@lib/agent-ui-contracts';
import { useEffect, useState } from 'react';
import {
  Animated,
  Easing,
  Modal,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  useWindowDimensions,
  View,
} from 'react-native';
import { useReducedMotionEnabled } from './useReducedMotion';
import { homePalettes, useOrcaScheme } from './appearance';

type Props = Pick<
  SessionHeaderProps,
  | 'workspace'
  | 'directory'
  | 'branch'
  | 'connected'
  | 'onReconnect'
  | 'topInset'
>;

export function SessionWorkspaceTitle({
  workspace,
  directory,
  branch,
  connected,
  onReconnect,
  topInset,
}: Props) {
  const colors = homePalettes[useOrcaScheme()];
  const { width, height } = useWindowDimensions();
  const [phase, setPhase] = useState<'closed' | 'open' | 'closing'>('closed');
  const [progress] = useState(() => new Animated.Value(0));
  const reducedMotion = useReducedMotionEnabled();
  useEffect(() => {
    if (phase === 'closed') return;
    const animation = Animated.timing(progress, {
      toValue: phase === 'open' ? 1 : 0,
      duration: 150,
      easing: Easing.out(Easing.cubic),
      useNativeDriver: Platform.OS !== 'web',
    });
    animation.start(({ finished }) => {
      if (finished && phase === 'closing') setPhase('closed');
    });
    return () => animation.stop();
  }, [phase, progress]);
  const [titleWidth, setTitleWidth] = useState(0);
  const cardWidth = Math.min(256, Math.max(0, width - 16));
  const left = Math.max(
    8,
    Math.min((width - titleWidth) / 2, width - cardWidth - 8)
  );
  const top = topInset + 52;
  const close = () => setPhase('closing');
  return (
    <View style={styles.center} pointerEvents="box-none">
      <View style={styles.trigger}>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={workspace}
          accessibilityHint="Shows the full workspace name and where it is"
          accessibilityState={{ expanded: phase === 'open' }}
          style={styles.trigger}
          onPress={() => setPhase('open')}
        >
          <Text
            numberOfLines={1}
            onLayout={(e) => setTitleWidth(e.nativeEvent.layout.width)}
            style={[styles.title, { color: colors.textPrimary }]}
          >
            {workspace}
          </Text>
          <View style={styles.meta}>
            <View
              style={[
                styles.dot,
                {
                  backgroundColor: connected
                    ? colors.statusGreen
                    : colors.statusAmber,
                },
              ]}
            />
            <Text
              numberOfLines={1}
              style={[styles.subtitle, { color: colors.textSecondary }]}
            >
              {connected ? (branch ?? 'Connected') : 'Connecting…'}
            </Text>
          </View>
        </Pressable>
        {!connected && (
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Reconnect to desktop"
            onPress={onReconnect}
            style={styles.retry}
          />
        )}
      </View>
      {phase !== 'closed' && (
        <Modal
          transparent
          visible
          onRequestClose={close}
          animationType="none"
          statusBarTranslucent
        >
          <View style={styles.overlay}>
            <Pressable
              accessibilityRole="button"
              accessibilityLabel="Close workspace details"
              onPress={close}
              style={StyleSheet.absoluteFill}
            />
            <Animated.View
              role="dialog"
              accessibilityLabel={workspace}
              style={[
                styles.card,
                {
                  opacity: progress,
                  transform:
                    reducedMotion === false
                      ? [
                          {
                            translateY: progress.interpolate({
                              inputRange: [0, 1],
                              outputRange: [-6, 0],
                            }),
                          },
                        ]
                      : [],
                  left,
                  top,
                  width: cardWidth,
                  maxHeight: Math.max(0, height - top - 8),
                  backgroundColor: colors.bgRaised,
                  borderColor: colors.borderSubtle,
                },
              ]}
            >
              <ScrollView
                keyboardShouldPersistTaps="always"
                contentContainerStyle={styles.content}
              >
                <Text
                  selectable
                  style={[styles.cardTitle, { color: colors.textPrimary }]}
                >
                  {workspace}
                </Text>
                {!!directory && (
                  <Text
                    selectable
                    style={[styles.directory, { color: colors.textSecondary }]}
                  >
                    {directory}
                  </Text>
                )}
              </ScrollView>
            </Animated.View>
          </View>
        </Modal>
      )}
    </View>
  );
}

const styles = StyleSheet.create({
  center: {
    ...StyleSheet.absoluteFill,
    justifyContent: 'center',
    alignItems: 'center',
    paddingHorizontal: 64,
  },
  trigger: { maxWidth: '100%', alignItems: 'center' },
  title: {
    fontSize: 16,
    fontWeight: '600',
    lineHeight: Platform.select({ ios: 20 }),
  },
  meta: { flexDirection: 'row', alignItems: 'center', gap: 8, marginTop: 2 },
  dot: { width: 8, height: 8, borderRadius: 4 },
  retry: { position: 'absolute', left: 0, right: 0, bottom: 0, height: 16 },
  subtitle: {
    flexShrink: 1,
    fontSize: 12,
    lineHeight: Platform.select({ ios: 15 }),
  },
  overlay: { flex: 1 },
  card: {
    position: 'absolute',
    borderWidth: 1,
    borderRadius: 16,
    shadowColor: '#000',
    shadowOpacity: 0.35,
    shadowRadius: 12,
    shadowOffset: { width: 0, height: 6 },
    elevation: 8,
  },
  content: { padding: 12 },
  cardTitle: {
    fontSize: 14,
    fontWeight: '600',
    lineHeight: Platform.select({ ios: 18 }),
  },
  directory: {
    fontSize: 12,
    lineHeight: Platform.select({ ios: 15 }),
    marginTop: 4,
  },
});
