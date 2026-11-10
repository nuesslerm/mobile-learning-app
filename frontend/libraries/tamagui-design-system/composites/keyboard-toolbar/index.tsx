import { Logger } from '@lib/logging';
import type { LucideIcon } from 'lucide-react-native';
import type { ReactNode } from 'react';
import {
  createContext,
  forwardRef,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import type { ScrollViewProps } from 'react-native';
import { Platform, ScrollView } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { useKeyboardFrame } from '../../lib/keyboard/useKeyboardFrame';
import { useOpenSheetStore } from '../../lib/openSheetStore';
import { XStack, YStack } from '../../primitives';
import { Icon } from '../../ui/icon';
import { Text } from '../../ui/text';

import { KeyboardToolbarPortal } from './portal';

/**
 * The keyboard toolbar: a rail of buttons parked directly above the soft
 * keyboard, and a scroll view that keeps its content clear of both.
 *
 * Roughly a third of app #1's 505-line version. Dropped along the way: the
 * `GuardedPortal` indirection (a plain `react-dom` portal on web and an
 * in-tree absolute on native says the same thing), `DebugOverlayNode` on
 * every node, and the rail's hand-rolled pointer-drag-to-scroll shim.
 *
 * Kept, because each earns it: the toolbar's measured height feeding the
 * scroll view's bottom padding (guessing it leaves a visible gap), the
 * keyboard-height `bottom` offset (the window never resizes for the keyboard,
 * edge-to-edge on Android included), and the deliberate
 * absence of a KeyboardAvoidingView -- app #1 documents that pairing it with
 * this padding pushes the last child a full keyboard-height above the toolbar.
 */

/**
 * The rail's stacking band. Exported so an app-level overlay can sit
 * deliberately above or below it rather than guessing a number.
 */
export const KEYBOARD_TOOLBAR_Z = 2000;

export type KeyboardToolbarRenderContext = {
  activeId: string | null;
  keyboardHeight: number;
  toolbarHeight: number;
};

type ContextValue = {
  activeId: string | null;
  keyboardVisible: boolean;
  keyboardHeight: number;
  toolbarHeight: number;
  setActiveId: (id: string | null) => void;
  setToolbarHeight: (height: number) => void;
};

const KeyboardToolbarContext = createContext<ContextValue | null>(null);

function useKeyboardToolbarContext(caller: string): ContextValue {
  const context = useContext(KeyboardToolbarContext);
  if (!context)
    throw new Error(`${caller} must be used inside a KeyboardToolbarTarget`);
  return context;
}

export type KeyboardToolbarTargetProps = {
  children: ReactNode;
  toolbar: (context: KeyboardToolbarRenderContext) => ReactNode;
  testID: string;
  toolbarTestID?: string;
};

export function KeyboardToolbarTarget({
  children,
  toolbar,
  testID,
  toolbarTestID = 'KeyboardToolbar',
}: KeyboardToolbarTargetProps) {
  const frame = useKeyboardFrame();
  // On Android (edge-to-edge) the reported keyboard height leaves out the
  // navigation bar the keyboard also covers (429 reported vs 466 on screen on
  // a 540x960 emulator), so the rail would still overlap the keyboard's top.
  const navBarInset = useSafeAreaInsets().bottom;
  const keyboard = useMemo(
    () => ({
      ...frame,
      height:
        frame.visible && Platform.OS === 'android'
          ? frame.height + navBarInset
          : frame.height,
    }),
    [frame, navBarInset]
  );
  const [activeId, setActiveId] = useState<string | null>(null);
  const [toolbarHeight, setToolbarHeightState] = useState(0);

  const setToolbarHeight = useCallback((height: number) => {
    setToolbarHeightState((previous) => {
      const rounded = Math.max(0, Math.round(height));
      if (previous === rounded) return previous;
      Logger.debug('keyboardToolbar.height', { height: rounded });
      return rounded;
    });
  }, []);

  const value = useMemo<ContextValue>(
    () => ({
      activeId,
      keyboardVisible: keyboard.visible,
      keyboardHeight: keyboard.height,
      toolbarHeight,
      setActiveId,
      setToolbarHeight,
    }),
    [
      activeId,
      keyboard.height,
      keyboard.visible,
      setToolbarHeight,
      toolbarHeight,
    ]
  );

  // A sheet on screen outranks the rail, and it has to be settled here rather
  // than by stacking order: this rail is a body-level portal while a sheet
  // renders inside its route, so the sheet can never win on z-index.
  const sheetOpen = useOpenSheetStore((state) => state.count > 0);

  return (
    <KeyboardToolbarContext.Provider value={value}>
      <YStack flex={1} position="relative" testID={testID}>
        {children}
        {keyboard.visible && !sheetOpen ? (
          <KeyboardToolbarPortal>
            <YStack
              testID={toolbarTestID}
              position={Platform.OS === 'web' ? 'fixed' : 'absolute'}
              l={0}
              r={0}
              // Above the debug FAB (z 1000). The FAB renders after the
              // navigator, so at an equal z it wins on paint order and covers
              // the rail's right-hand keys -- which are the Rename/Done end of
              // the action row, i.e. the ones worth reaching.
              z={KEYBOARD_TOOLBAR_Z}
              // The window does not resize for the keyboard (Android runs
              // edge-to-edge, so adjustResize is ignored): at b=0 the rail sat
              // under the keyboard (IME frame top 494, rail 808-960 on a 540x960
              // emulator). Lift it by the keyboard height on every platform.
              b={keyboard.height}
              onLayout={(event) =>
                setToolbarHeight(event.nativeEvent.layout.height)
              }
            >
              {toolbar({
                activeId,
                keyboardHeight: keyboard.height,
                toolbarHeight,
              })}
            </YStack>
          </KeyboardToolbarPortal>
        ) : null}
      </YStack>
    </KeyboardToolbarContext.Provider>
  );
}

export type UseKeyboardToolbarTargetOptions = { id: string };

export function useKeyboardToolbarTarget({
  id,
}: UseKeyboardToolbarTargetOptions) {
  const context = useKeyboardToolbarContext('useKeyboardToolbarTarget');
  const blurTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const clearBlurTimer = useCallback(() => {
    if (blurTimerRef.current) {
      clearTimeout(blurTimerRef.current);
      blurTimerRef.current = null;
    }
  }, []);

  useEffect(() => clearBlurTimer, [clearBlurTimer]);

  const activate = useCallback(() => {
    clearBlurTimer();
    context.setActiveId(id);
  }, [clearBlurTimer, context, id]);

  // Deferred, not immediate: tapping a toolbar button blurs the input for a
  // frame, and clearing the active id synchronously would tear the toolbar
  // down under the finger that is pressing it.
  const deactivate = useCallback(() => {
    clearBlurTimer();
    blurTimerRef.current = setTimeout(() => {
      if (context.activeId === id) context.setActiveId(null);
    }, 120);
  }, [clearBlurTimer, context, id]);

  return {
    active: context.activeId === id,
    activeId: context.activeId,
    toolbarVisible: context.activeId === id && context.keyboardVisible,
    keyboardHeight: context.keyboardHeight,
    toolbarHeight: context.toolbarHeight,
    onFocus: activate,
    onBlur: deactivate,
    activate,
    deactivate,
  };
}

export type KeyboardToolbarAwareScrollViewProps = ScrollViewProps & {
  bottomInset?: number;
  growContentContainer?: boolean;
};

export const KeyboardToolbarAwareScrollView = forwardRef<
  ScrollView,
  KeyboardToolbarAwareScrollViewProps
>(function KeyboardToolbarAwareScrollView(
  {
    bottomInset = 24,
    contentContainerStyle,
    keyboardShouldPersistTaps = 'handled',
    growContentContainer = true,
    style,
    ...props
  },
  forwardedRef
) {
  const context = useKeyboardToolbarContext('KeyboardToolbarAwareScrollView');
  const keyboardInset = context.keyboardVisible ? context.keyboardHeight : 0;
  const toolbarInset = context.keyboardVisible ? context.toolbarHeight : 0;

  return (
    <ScrollView
      ref={forwardedRef}
      keyboardShouldPersistTaps={keyboardShouldPersistTaps}
      keyboardDismissMode="none"
      showsVerticalScrollIndicator={false}
      style={[{ flex: 1 }, style]}
      {...props}
      contentContainerStyle={[
        growContentContainer ? { flexGrow: 1 } : null,
        contentContainerStyle,
        // The toolbar covers the bottom safe area when it is up, so adding
        // bottomInset on top of the keyboard inset double-counts it and opens
        // a gap between the last child and the rail.
        {
          paddingBottom:
            keyboardInset > 0 ? keyboardInset + toolbarInset : bottomInset,
        },
      ]}
    />
  );
});

export type KeyboardToolbarRailProps = {
  children: ReactNode;
  testID: string;
};

export function KeyboardToolbarRail({
  children,
  testID,
}: KeyboardToolbarRailProps) {
  return (
    <YStack
      bg="$color2"
      borderTopWidth={1}
      borderColor="$borderColor"
      testID={testID}
    >
      <ScrollView
        horizontal
        keyboardShouldPersistTaps="always"
        showsHorizontalScrollIndicator={false}
        contentContainerStyle={{
          paddingHorizontal: 8,
          paddingVertical: 6,
          gap: 6,
        }}
      >
        {children}
      </ScrollView>
    </YStack>
  );
}

export type KeyboardToolbarButtonProps = {
  label: string;
  accessibilityLabel: string;
  onPress: () => void;
  testID: string;
  icon?: LucideIcon;
  disabled?: boolean;
  tone?: 'default' | 'accent' | 'success' | 'danger';
};

const TONE_COLOR = {
  default: '$color',
  accent: '$blue10',
  success: '$green10',
  danger: '$red10',
} as const;

export function KeyboardToolbarButton({
  label,
  accessibilityLabel,
  onPress,
  testID,
  icon,
  disabled = false,
  tone = 'default',
}: KeyboardToolbarButtonProps) {
  return (
    <XStack
      testID={testID}
      role="button"
      aria-label={accessibilityLabel}
      items="center"
      gap="$1.5"
      px="$2.5"
      height={38}
      rounded="$3"
      bg="$color3"
      opacity={disabled ? 0.4 : 1}
      pressStyle={{ bg: '$color5' }}
      cursor={disabled ? 'default' : 'pointer'}
      onPress={disabled ? undefined : onPress}
    >
      {icon ? <Icon as={icon} size={16} color={TONE_COLOR[tone]} /> : null}
      <Text variant="small" color={TONE_COLOR[tone]}>
        {label}
      </Text>
    </XStack>
  );
}
