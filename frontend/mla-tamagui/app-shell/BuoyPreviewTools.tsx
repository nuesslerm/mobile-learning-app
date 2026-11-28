import { FloatingDevTools } from '@buoy-gg/core';
import { useProAccess } from '@buoy-gg/license';
import {
  AccountSetupButton,
  BuoyNeedsPlanCard,
  BuoySignInCard,
  useSignInState,
} from '@buoy-gg/shared-ui';
import { useMemo, useRef, useState } from 'react';
import { PanResponder, Pressable, StyleSheet, View, useWindowDimensions } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { useOpenSheetStore, usePresetThemeStore } from '@lib/tamagui-design-system';

const stores = { theme: usePresetThemeStore, sheets: useOpenSheetStore };
const clamp = (n: number, limit: number) => Math.max(0, Math.min(n, limit));

function BuoyAccountControl() {
  const [open, setOpen] = useState(false);
  const needsPlan = useSignInState() === 'active';
  const screen = useWindowDimensions();
  const inset = useSafeAreaInsets();
  const [size, measure] = useState({ width: 0, height: 0 });
  const [position, move] = useState<{ x: number; y: number }>();
  const maxX = Math.max(0, screen.width - inset.left - inset.right - size.width);
  const maxY = Math.max(0, screen.height - inset.top - inset.bottom - size.height);
  const x = clamp(position?.x ?? maxX - 16, maxX);
  const y = clamp(position?.y ?? maxY - 8, maxY);
  const start = useRef({ x, y });
  const current = useRef({ x, y, maxX, maxY });
  current.current = { x, y, maxX, maxY };
  const pan = useMemo(() => PanResponder.create({
    onMoveShouldSetPanResponderCapture: (_, g) => Math.hypot(g.dx, g.dy) > 5,
    onPanResponderGrant: () => { start.current = current.current; },
    onPanResponderMove: (_, g) => move({
      x: clamp(start.current.x + g.dx, current.current.maxX),
      y: clamp(start.current.y + g.dy, current.current.maxY),
    }),
    onPanResponderTerminationRequest: () => false,
  }), []);
  return (
    <View style={StyleSheet.absoluteFill} pointerEvents="box-none">
      <View
        {...pan.panHandlers}
        onLayout={({ nativeEvent: { layout } }) => measure({ width: layout.width, height: layout.height })}
        style={{ position: 'absolute', right: inset.right, bottom: inset.bottom, transform: [{ translateX: x - maxX }, { translateY: y - maxY }] }}
      >
        <AccountSetupButton expanded={open} needsPlan={needsPlan} onPress={() => setOpen(true)} />
      </View>
      {open && (
        <View accessibilityViewIsModal style={styles.scrim}>
          <Pressable style={StyleSheet.absoluteFill} accessibilityLabel="Dismiss Buoy" onPress={() => setOpen(false)} />
          {needsPlan
            ? <BuoyNeedsPlanCard onClose={() => setOpen(false)} />
            : <BuoySignInCard onClose={() => setOpen(false)} />}
        </View>
      )}
    </View>
  );
}

export function BuoyPreviewTools() {
  const { isLicensed } = useProAccess();
  const accountOnly = !__DEV__ && !isLicensed;
  return (
    <>
      <FloatingDevTools licenseKey={process.env.EXPO_PUBLIC_BUOY_KEY || undefined} signIn={!__DEV__} headless={accountOnly} zustandStores={stores} />
      {accountOnly && <BuoyAccountControl />}
    </>
  );
}

const styles = StyleSheet.create({
  scrim: { ...StyleSheet.absoluteFill, justifyContent: 'center', alignItems: 'center', padding: 20, backgroundColor: 'rgba(0,0,0,0.65)' },
});
