import BottomSheet, {
  BottomSheetBackdrop,
  BottomSheetFlatList,
  BottomSheetScrollView,
  type BottomSheetBackdropProps,
  type BottomSheetBackgroundProps,
} from '@gorhom/bottom-sheet';
import { Logger } from '@lib/logging';
import { GlassView, isGlassEffectAPIAvailable, isLiquidGlassAvailable, type GlassColorScheme } from 'expo-glass-effect';
import type { ReactNode } from 'react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Keyboard,
  Platform,
  Pressable,
  StyleSheet,
  Text as RNText,
  View,
  type ColorValue,
  type StyleProp,
  type ViewStyle,
} from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { X } from '../icons';
import { useOpenSheetStore } from '../lib/openSheetStore';
import { useKeyboardFrame } from '../lib/keyboard/useKeyboardFrame';
import { useTheme } from '../primitives';

import { Icon } from './icon';

export type SheetProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  children: ReactNode;
  /** Percentages of screen height, largest first. */
  snapPoints?: number[];
  topInset?: number;
  floating?: boolean;
  floatingHorizontalInset?: number;
  /** Distance from the window's bottom edge for a floating sheet. Defaults to the safe area plus 8 points. */
  floatingBottomInset?: number;
  cornerRadius?: number;
  glassColorScheme?: GlassColorScheme;
  glassTintColor?: ColorValue;
  /** Controls whether the sheet itself is a single accessibility element. */
  accessible?: boolean;
  /** Keep the bottom inset fixed. A form's scroll view handles the keyboard overlap. */
  keyboardOverlay?: boolean;
  /**
   * Hands the vertical drag to whatever the body renders instead of the sheet.
   * Pass it when the body holds a scrollable that is NOT one of gorhom's own
   * (`Sheet.ScrollView`/`Sheet.FlatList`) -- a third-party wheel, say. Without
   * it the sheet's pan and that scrollable both claim the touch and the inner
   * one loses. The handle and the backdrop still dismiss.
   */
  disableContentPanningGesture?: boolean;
  /**
   * 'dim' (default) is a modal sheet with a tappable dimmed layer. 'none' draws no layer and lets touches through the container to
   * what is behind the sheet: a non-modal panel the page above it stays usable beside.
   */
  backdrop?: 'dim' | 'none';
  backdropOpacity?: number;
  /** Makes the drag handle a button with this label (`${testID}Handle`) that closes the sheet and exposes a dismiss action. */
  handleLabel?: string;
  handleIndicatorStyle?: StyleProp<ViewStyle>;
  contentStyle?: StyleProp<ViewStyle>;
  testID: string;
};

/**
 * Bottom sheet on @gorhom/bottom-sheet's INLINE `BottomSheet` export.
 *
 * Inline, not `BottomSheetModal`, and that is the whole point: the modal
 * variant portals to a host, and a portalled sheet inside a route that is
 * already `presentation: 'transparentModal'` renders correctly and then
 * swallows every touch -- the bug that shipped twice here. The inline export
 * renders in the presented view controller's own subtree, so that failure is
 * designed out rather than guarded against, and the `modal` prop this
 * component used to need is gone.
 *
 * Every default below differs from gorhom's own. They are not preferences:
 * gorhom ships `enablePanDownToClose: false` and a backdrop that only appears
 * at snap index 1, so a stock single-snap-point sheet has no drag dismissal
 * AND no backdrop to tap -- no exit at all. App #1 has run this corrected set
 * on a real device for months.
 */
/**
 * False on the first render, true from the second.
 *
 * gorhom measures the window to lay the sheet out, so rendering it during the
 * static export's prerender produces markup that cannot match what the real
 * viewport hydrates to -- React #418, thrown on every sheet route. Mounting a
 * frame later costs nothing visible and keeps the sheet out of hydration
 * entirely.
 */
function useMountedAfterFirstFrame(): boolean {
  const [mounted, setMounted] = useState(false);
  useEffect(() => {
    setMounted(true);
  }, []);
  return mounted;
}

/**
 * The dimmed layer behind the sheet, and the only tappable way out of one that
 * has no close button.
 *
 * No testID: BottomSheetBackdrop does not forward one to the DOM. It does set
 * accessibilityRole/Label ("Bottom sheet backdrop"), which is the stable handle
 * checks assert on -- and a better one, since it is the same contract a screen
 * reader uses.
 *
 * appearsOnIndex 0, not gorhom's default of 1: at a single snap point the sheet
 * sits at index 0, where the stock backdrop never renders -- leaving nothing to
 * tap and no way out.
 */
function renderBackdrop(props: BottomSheetBackdropProps, opacity?: number) {
  return (
    <BottomSheetBackdrop
      {...props}
      opacity={opacity}
      appearsOnIndex={0}
      disappearsOnIndex={-1}
      pressBehavior="close"
    />
  );
}

/**
 * Publishes "a sheet is up" for the whole app, for as long as `open` is true.
 * The keyboard toolbar rail reads this and unmounts; see openSheetStore for why
 * a z-index cannot do that job.
 */
export function usePublishSheetOpen(open: boolean): void {
  useEffect(() => {
    if (!open) return;
    const { open: openSheet, close: closeSheet } = useOpenSheetStore.getState();
    openSheet();
    return closeSheet;
  }, [open]);
}

/**
 * Puts the keyboard away for as long as `open` is true.
 *
 * Both calls are needed. `Keyboard.dismiss()` resigns the first responder,
 * which is what closes a real soft keyboard (including one raised by the
 * editor's WebView). On web it only blurs React Native's own tracked
 * TextInput, so a focused CodeMirror iframe keeps focus and the keyboard state
 * stays "up" -- hence the explicit blur, which is also what makes this
 * assertable in a browser check.
 */
function useDismissKeyboardWhileOpen(open: boolean): void {
  useEffect(() => {
    if (!open) return;
    Keyboard.dismiss();
    if (Platform.OS !== 'web' || typeof document === 'undefined') return;
    const active = document.activeElement;
    if (active instanceof HTMLElement) active.blur();
  }, [open]);
}

function LabeledHandle({ label, testID, indicatorStyle, onDismiss }: {
  label: string;
  testID: string;
  indicatorStyle: StyleProp<ViewStyle>;
  onDismiss: () => void;
}) {
  return (
    <Pressable
      testID={`${testID}Handle`}
      accessibilityRole="button"
      aria-label={label}
      accessibilityActions={[{ name: 'dismiss', label }]}
      onAccessibilityAction={(event) => {
        if (event.nativeEvent.actionName === 'dismiss') onDismiss();
      }}
      onPress={onDismiss}
      hitSlop={8}
      style={styles.handle}
    >
      <View style={[styles.handleBar, indicatorStyle]} />
    </Pressable>
  );
}

export function Sheet({
  open,
  onOpenChange,
  children,
  snapPoints = [85, 50],
  topInset,
  floating = false,
  floatingHorizontalInset = 12,
  floatingBottomInset,
  cornerRadius,
  glassColorScheme,
  glassTintColor,
  accessible,
  keyboardOverlay = false,
  disableContentPanningGesture = false,
  backdrop = 'dim',
  backdropOpacity,
  handleLabel,
  handleIndicatorStyle,
  contentStyle,
  testID,
}: SheetProps) {
  const theme = useTheme();
  const ref = useRef<BottomSheet>(null);
  const useGlass = Platform.OS === 'ios' && isLiquidGlassAvailable() && isGlassEffectAPIAvailable();
  // gorhom computes the sheet's total available height from the LAST entry
  // of the `snapPoints` array it is given (`highestDetentPosition =
  // detents[detents.length - 1]`), which only lands on the tallest point when
  // the array is ascending. Our own prop is documented "largest first" (index
  // 0 = the default open state), so re-sort ascending before handing gorhom
  // the array, and track where the largest point landed to open to it.
  //
  // Passing them in as given (descending, e.g. [90, 60]) silently undersizes
  // the sheet's content area by the gap between the two points -- for
  // [90, 60] on an 852pt screen that is 0.3 * 852 = 256px of dead space below
  // the content, matching exactly what shipped.
  const points = useMemo(
    () => [...snapPoints].sort((a, b) => a - b).map((p) => `${p}%`),
    [snapPoints]
  );
  const openIndex = points.length - 1;

  // `index` is derived from `open` in the SAME render. The tempting
  // alternative -- a `useEffect([open])` that calls `ref.open()` -- races
  // gorhom's own `index` prop and silently fails to re-open on the first
  // toggle, which reads as "the sheet is just broken sometimes".
  const index = open ? openIndex : -1;

  // Close the loop the other way: a pan-down or backdrop dismiss must be
  // written back to the caller's state, or the sheet animates away and then
  // parks off-screen forever because the route never pops.
  //
  // This is gorhom's dedicated `onClose`, NOT an `onChange(-1)` check. Its
  // `onChange` only fires when the settled index DIFFERS from the current one
  // (`if (nextIndex !== animatedCurrentIndex.get())`), while `onClose` is
  // called unconditionally at -1 -- so inferring closure from onChange drops
  // exactly the dismissals that matter.
  const handleClose = useCallback(() => {
    Logger.info('ds.sheet.dismissed', { testID });
    onOpenChange(false);
  }, [onOpenChange, testID]);

  useEffect(() => {
    if (open) ref.current?.snapToIndex(openIndex);
    else ref.current?.close();
  }, [open, openIndex]);

  usePublishSheetOpen(open);

  // An open sheet is modal, so the keyboard belongs to whatever is behind it --
  // on a device it otherwise sits ON TOP of the sheet, covering the very
  // control the sheet exists to offer.
  useDismissKeyboardWhileOpen(open);

  // The sheet's bottom edge goes up to the keyboard's top (`bottomInset`). Opening a sheet puts the keyboard away, so a keyboard that
  // rises while one is open is for a field in it. gorhom's own lift is not relied on: it only acts for its own input, and on the CI
  // emulator it did not move the sheet at all (Expo Go, edge to edge); the keyboard source is the one the toolbar rail follows. The same
  // code runs on web, where the browser tests and the flow tests see it.
  //
  // The sheet gets smaller by the keyboard's height (its percent points are of the room that is left), so a sheet with a field should
  // be tall enough for that room: VoiceReviewSheet takes 95% in its text mode. Changing the snap points while the sheet is open is not
  // an option: it takes the focus from the field (the keyboard goes down again).
  const keyboard = useKeyboardFrame();
  const insets = useSafeAreaInsets();
  // Android's keyboard events leave out the navigation bar that the keyboard also covers (the toolbar rail adds it the same way), and
  // the sheet's container ends at the bottom of the window
  const keyboardLift = open && keyboard.visible && !keyboardOverlay
    ? keyboard.height + (Platform.OS === 'android' ? insets.bottom : 0)
    : 0;
  const bottomInset = floating
    ? Math.max(keyboardLift, floatingBottomInset ?? insets.bottom + 8)
    : keyboardLift;

  const mounted = useMountedAfterFirstFrame();
  const indicatorColor = theme.color8?.val ?? '#999';
  const renderHandle = useCallback(
    () => (
      <LabeledHandle
        label={handleLabel ?? ''}
        testID={testID}
        indicatorStyle={[{ backgroundColor: indicatorColor }, handleIndicatorStyle]}
        onDismiss={handleClose}
      />
    ),
    [handleLabel, testID, indicatorColor, handleIndicatorStyle, handleClose]
  );
  const renderGlassBackground = useCallback(
    ({ style }: BottomSheetBackgroundProps) => (
      <GlassView
        glassEffectStyle="regular"
        colorScheme={glassColorScheme}
        tintColor={glassTintColor}
        style={[
          style,
          styles.glassBackground,
          cornerRadius === undefined ? null : {
            borderRadius: cornerRadius,
            borderTopLeftRadius: cornerRadius,
            borderTopRightRadius: cornerRadius,
          },
        ]}
      />
    ),
    [cornerRadius, glassColorScheme, glassTintColor]
  );
  const renderDimBackdrop = useCallback(
    (props: BottomSheetBackdropProps) => renderBackdrop(props, backdropOpacity),
    [backdropOpacity]
  );

  if (!mounted || !open) return null;

  return (
    <BottomSheet
      ref={ref}
      accessible={accessible}
      index={index}
      snapPoints={points}
      onClose={handleClose}
      // gorhom default is false -- without this the sheet cannot be swiped away.
      enablePanDownToClose
      enableContentPanningGesture={!disableContentPanningGesture}
      // Dynamic sizing fights explicit snap points; app #1 hardcodes it off.
      enableDynamicSizing={false}
      backdropComponent={backdrop === 'none' ? undefined : renderDimBackdrop}
      // gorhom's web container is a full-size absolute div that would swallow every tap meant for what is behind a sheet with no backdrop
      {...(backdrop === 'none' ? { containerStyle: styles.passThrough } : {})}
      {...(handleLabel ? { handleComponent: renderHandle } : {})}
      keyboardBehavior={keyboardOverlay ? 'extend' : 'interactive'}
      // gorhom default is 'none', which leaves the keyboard up over the sheet.
      keyboardBlurBehavior="restore"
      topInset={topInset}
      bottomInset={bottomInset}
      detached={floating}
      style={floating ? { marginHorizontal: floatingHorizontalInset } : undefined}
      android_keyboardInputMode="adjustResize"
      // color2, NOT `background`: a sheet painted in the page's own background
      // is invisible on the dark theme -- on a device the only things that read
      // as a sheet at all were the handle and one hairline divider, over what
      // looked like empty screen. This is the same elevated surface the header
      // menu's card uses.
      backgroundComponent={useGlass ? renderGlassBackground : undefined}
      backgroundStyle={[
        !useGlass && { backgroundColor: theme.color2?.val ?? '#fff' },
        floating && styles.floatingBackground,
        cornerRadius === undefined ? null : { borderRadius: cornerRadius },
      ]}
      handleIndicatorStyle={[
        { backgroundColor: floating ? 'rgba(127, 127, 127, 0.65)' : indicatorColor },
        handleIndicatorStyle,
      ]}
      // RNGH's web pan handler drops its default min-distance activation as
      // soon as ANY custom offset is set, so a sheet configured with only a
      // horizontal fail-offset ends up with no activation criterion at all and
      // cannot be dragged in a browser. Restoring the 15px vertical window
      // matches RNGH's own touch slop. Native is unaffected.
      {...(Platform.OS === 'web'
        ? { activeOffsetY: [-15, 15] as [number, number] }
        : {})}
    >
      <View testID={testID} style={[styles.frame, contentStyle]}>
        {children}
      </View>
    </BottomSheet>
  );
}

// Re-exported so a callsite never reaches for a plain RN ScrollView. gorhom's
// scrollables register with the sheet's gesture handler; a plain one does not,
// so the list scroll and the sheet pan both claim the touch and neither works.
Sheet.ScrollView = BottomSheetScrollView;
Sheet.FlatList = BottomSheetFlatList;

export type SheetHeaderProps = {
  children: ReactNode;
  /**
   * Renders a close control on the right. Pass it whenever the sheet is the
   * only thing on screen: gesture dismissal is invisible, and a sheet a user
   * cannot get out of is a trapped app, not a styling nit.
   */
  onClose?: () => void;
  closeAccessibilityLabel?: string;
  /** Beside the title on the left (a Reset pill). With either slot the row centres its items vertically. */
  leading?: ReactNode;
  /** Beside the title on the right, before the close control (a Done pill). */
  trailing?: ReactNode;
  testID: string;
  style?: StyleProp<ViewStyle>;
};

/** A non-scrolling header pinned above the sheet's scrollable body. */
export function SheetHeader({
  children,
  onClose,
  closeAccessibilityLabel = 'Close',
  leading,
  trailing,
  testID,
  style,
}: SheetHeaderProps) {
  const theme = useTheme();
  const border = theme.borderColor?.val ?? 'rgba(128,128,128,0.35)';
  const chip = theme.color3?.val ?? 'rgba(128,128,128,0.22)';
  const hasSlots = leading != null || trailing != null;

  return (
    <View
      testID={testID}
      style={[
        styles.header,
        hasSlots && styles.headerWithSlots,
        { borderBottomColor: border },
        style,
      ]}
    >
      {leading != null ? <View style={styles.slot}>{leading}</View> : null}
      <View style={styles.headerBody}>{children}</View>
      {trailing != null ? <View style={styles.slot}>{trailing}</View> : null}
      {onClose ? (
        <Pressable
          testID={`${testID}Close`}
          accessibilityRole="button"
          aria-label={closeAccessibilityLabel}
          onPress={() => {
            Logger.info('ds.sheet.close_tap', { testID });
            onClose();
          }}
          // Bigger than the visual circle: a 32px control is under the 44px
          // minimum touch target, and this is the only guaranteed way out.
          hitSlop={12}
          style={({ pressed }) => [
            styles.close,
            { backgroundColor: chip },
            pressed && { opacity: 0.6 },
          ]}
        >
          <Icon as={X} size={18} />
        </Pressable>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create({
  passThrough: { pointerEvents: 'box-none' },
  floatingBackground: { borderRadius: 28 },
  glassBackground: { pointerEvents: 'none', borderTopLeftRadius: 28, borderTopRightRadius: 28 },
  handle: { alignItems: 'center', justifyContent: 'center', paddingTop: 10, paddingBottom: 10 },
  handleBar: { width: 36, height: 4, borderRadius: 2 },
  frame: { flex: 1, paddingHorizontal: 16, paddingTop: 4, gap: 12 },
  header: {
    flexDirection: 'row',
    alignItems: 'flex-start',
    gap: 12,
    paddingBottom: 10,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  headerWithSlots: { alignItems: 'center' },
  headerBody: { flex: 1, minWidth: 0 },
  slot: { flexShrink: 0 },
  close: {
    width: 32,
    height: 32,
    borderRadius: 16,
    alignItems: 'center',
    justifyContent: 'center',
  },
});

/** Re-exported so a sheet body can use plain RN text without importing both. */
export const SheetText = RNText;
