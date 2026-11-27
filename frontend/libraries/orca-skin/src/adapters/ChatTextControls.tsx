import { useRef, useState } from 'react';
import {
  Platform,
  Pressable,
  StyleSheet,
  Text,
  View,
  type GestureResponderEvent,
  type ViewProps,
} from 'react-native';
import { homePalettes, useOrcaScheme } from './appearance';
import { useChatText } from './chatText';

export function ChatSizeControl({ label }: { label: string }) {
  const { value, defaultSize, update } = useChatText();
  const colors = homePalettes[useOrcaScheme()];
  const [width, setWidth] = useState(0);
  const start = useRef({ page: 0, local: 0 });
  const fraction = (value.size - 11) / 13;
  const travel = Math.max(0, width - 26);
  const select = (size: number) =>
    update({ size: Math.max(11, Math.min(24, Math.round(size))) });
  function drag(event: GestureResponderEvent) {
    const x =
      start.current.local + event.nativeEvent.pageX - start.current.page;
    if (travel) select(11 + (13 * (x - 13)) / travel);
  }
  const pointLabel =
    value.size === defaultSize ? 'Default' : value.size + ' pt';
  const webKeys: Pick<ViewProps, 'onKeyDown'> =
    Platform.OS === 'web'
      ? {
          onKeyDown: (event) => {
            const key = event.nativeEvent.key;
            const keys: Record<string, number> = {
              Home: 11,
              End: 24,
              ArrowLeft: value.size - 1,
              ArrowDown: value.size - 1,
              ArrowUp: value.size + 1,
              ArrowRight: value.size + 1,
            };
            if (key in keys) {
              event.preventDefault();
              select(keys[key]);
            }
          },
        }
      : {};
  return (
    <View style={styles.stacked}>
      <View style={styles.head}>
        <Text style={{ fontSize: 15, color: colors.textPrimary }}>{label}</Text>
        <Text style={{ fontSize: 15, color: colors.textSecondary }}>
          {pointLabel}
        </Text>
      </View>
      <View style={styles.sliderLine}>
        <Text style={{ fontSize: 13, color: colors.textSecondary }}>A</Text>
        <View
          accessibilityRole="adjustable"
          accessibilityLabel="Chat text size"
          accessible
          focusable
          accessibilityValue={{
            min: 11,
            max: 24,
            now: value.size,
            text: pointLabel,
          }}
          accessibilityActions={[{ name: 'increment' }, { name: 'decrement' }]}
          onAccessibilityAction={({ nativeEvent }) => {
            if (nativeEvent.actionName === 'increment') select(value.size + 1);
            if (nativeEvent.actionName === 'decrement') select(value.size - 1);
          }}
          onLayout={(e) => setWidth(e.nativeEvent.layout.width)}
          onStartShouldSetResponder={() => true}
          onResponderGrant={(e) => {
            start.current = {
              page: e.nativeEvent.pageX,
              local: e.nativeEvent.locationX,
            };
            drag(e);
          }}
          onResponderMove={drag}
          {...webKeys}
          style={styles.slider}
        >
          <View
            pointerEvents="none"
            style={[styles.track, { backgroundColor: colors.borderSubtle }]}
          >
            <View style={[styles.fill, { width: travel * fraction }]} />
          </View>
          <View
            pointerEvents="none"
            style={[styles.thumb, { left: travel * fraction }]}
          />
        </View>
        <Text style={{ fontSize: 20, color: colors.textSecondary }}>A</Text>
      </View>
    </View>
  );
}

export function ChatSpacingControl({ label = false }: { label?: boolean }) {
  const { value, update } = useChatText();
  const colors = homePalettes[useOrcaScheme()];
  return (
    <View style={styles.stacked}>
      {label && (
        <View style={styles.head}>
          <Text style={{ fontSize: 15, color: colors.textPrimary }}>
            Line spacing
          </Text>
        </View>
      )}
      <View style={styles.segments}>
        {(['compact', 'default', 'relaxed'] as const).map((lineSpacing) => {
          const selected = value.lineSpacing === lineSpacing;
          return (
            <Pressable
              key={lineSpacing}
              accessibilityRole="button"
              accessibilityState={{ selected }}
              style={[
                styles.segment,
                selected && { backgroundColor: colors.bgRaised },
              ]}
              onPress={() => update({ lineSpacing })}
            >
              <Text
                style={{
                  fontSize: 13,
                  fontWeight: selected ? '600' : '400',
                  color: colors.textPrimary,
                }}
              >
                {lineSpacing[0].toUpperCase() + lineSpacing.slice(1)}
              </Text>
            </Pressable>
          );
        })}
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  stacked: { paddingHorizontal: 16, paddingVertical: 8, gap: 4 },
  head: {
    minHeight: 32,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
  },
  sliderLine: { flexDirection: 'row', alignItems: 'center', gap: 8 },
  slider: { flex: 1, height: 44, justifyContent: 'center' },
  track: {
    height: 4,
    marginHorizontal: 13,
    borderRadius: 2,
    overflow: 'hidden',
  },
  fill: { height: 4, backgroundColor: '#007aff' },
  thumb: {
    position: 'absolute',
    top: 9,
    width: 26,
    height: 26,
    borderRadius: 13,
    backgroundColor: '#fff',
    shadowColor: '#000',
    shadowOpacity: 0.3,
    shadowRadius: 3,
    shadowOffset: { width: 0, height: 2 },
    elevation: 2,
  },
  segments: { flexDirection: 'row', gap: 4 },
  segment: {
    flex: 1,
    height: 34,
    borderRadius: 8,
    alignItems: 'center',
    justifyContent: 'center',
  },
});
