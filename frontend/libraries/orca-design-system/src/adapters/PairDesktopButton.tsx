import { useState } from 'react';
import { Pressable, Text } from 'react-native';
import Animated from 'react-native-reanimated';
import QrCodeIcon from 'lucide-react-native/dist/esm/icons/qr-code.mjs';
import type { PairDesktopButtonProps } from '@lib/agent-ui-contracts';
import { homePalettes, useOrcaScheme } from './appearance';
import { useHomeStyles } from './homeStyles';

export function PairDesktopButton(props: PairDesktopButtonProps) {
  const colors = homePalettes[useOrcaScheme()];
  const styles = useHomeStyles();
  const [pressed, setPressed] = useState(false);
  return (
    <Pressable
      accessibilityRole="button"
      style={styles.ctaHit}
      onPressIn={() => setPressed(true)}
      onPressOut={() => setPressed(false)}
      onPress={props.onPress}
    >
      <Animated.View
        style={[
          styles.cta,
          {
            opacity: pressed ? 0.85 : 1,
            transform: [{ scale: pressed ? 0.97 : 1 }],
            transitionProperty: ['transform', 'opacity'],
            transitionDuration: pressed ? 90 : 160,
          },
        ]}
      >
        <QrCodeIcon size={18} color={colors.onAccent} strokeWidth={2.2} />
        <Text style={styles.ctaLabel}>{props.label}</Text>
      </Animated.View>
    </Pressable>
  );
}
