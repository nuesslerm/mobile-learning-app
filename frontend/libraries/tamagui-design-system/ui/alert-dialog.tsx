import { createAlertDialog } from '@gluestack-ui/core/alert-dialog/creator';
import { OverlayProvider } from '@gluestack-ui/core/overlay/aria';
import type { ReactNode } from 'react';
import type { StyleProp, ViewStyle } from 'react-native';
import { Pressable, StyleSheet, View } from 'react-native';

import {
  Button,
  XStack,
  YStack,
  useTheme,
  type ButtonProps,
  type XStackProps,
  type YStackProps,
} from '../primitives';

import { Text, type TextProps } from './text';

type AlertDialogShellProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /**
   * Renders its own modal layer. Pass FALSE when a route already provides one
   * (`presentation: 'transparentModal'`).
   */
  modal?: boolean;
};

/**
 * The prop-driven confirm dialog: title, optional description, cancel and
 * confirm. Nothing composes into it.
 */
export type AlertDialogConfirmProps = AlertDialogShellProps & {
  title: string;
  description?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  destructive?: boolean;
  onConfirm: () => void;
  testID: string;
  children?: never;
};

/**
 * The composed form, for a dialog whose body is content rather than a
 * question: the caller supplies AlertDialogContent and the header/footer
 * subparts this module exports. Without this, those subparts had nothing to
 * compose into -- the type demanded `title` and `onConfirm` and rejected
 * `children`, so every composed use failed to typecheck.
 */
export type AlertDialogComposedProps = AlertDialogShellProps & {
  children: ReactNode;
  testID?: string;
};

export type AlertDialogProps =
  | AlertDialogConfirmProps
  | AlertDialogComposedProps;

/**
 * Slot components for gluestack's headless factory.
 *
 * They are plain RN primitives on purpose. gluestack supplies the BEHAVIOUR
 * (focus containment, escape/overlay dismissal, aria roles, keyboard inset)
 * and nothing else -- so the rendering stays ours, in primitives whose web and
 * native implementations are the same code path.
 *
 * Each slot drops gluestack's internal layout props (`exit`, `visible`) rather
 * than spreading them onto a View, where they would reach the DOM as unknown
 * attributes. It does NOT drop `style`: an earlier version did, which silently
 * discarded the card's own background and the backdrop's dim, so on a device
 * the dialog was bare text floating on the screen behind it with nothing
 * darkened -- it read as a rendering glitch rather than a dialog.
 */
type SlotProps = {
  children?: ReactNode;
  exit?: boolean;
  visible?: boolean;
  style?: StyleProp<ViewStyle>;
  [key: string]: unknown;
};

function makeSlot(baseStyle: ViewStyle) {
  return function Slot({
    children,
    exit: _exit,
    visible: _visible,
    style,
    ...rest
  }: SlotProps) {
    return (
      <View {...rest} style={[baseStyle, style]}>
        {children}
      </View>
    );
  };
}

const PassThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;

/**
 * React Native 0.86 removed `ABSOLUTE_FILL`. The literal it
 * always was works everywhere, including spread into another style object,
 * which the registered `StyleSheet.absoluteFill` id does not.
 */
const ABSOLUTE_FILL = {
  position: 'absolute',
  left: 0,
  right: 0,
  top: 0,
  bottom: 0,
} as const;

const GluestackAlertDialog = createAlertDialog({
  Root: makeSlot(ABSOLUTE_FILL),
  Content: makeSlot({}),
  CloseButton: Pressable,
  Header: makeSlot({}),
  Footer: makeSlot({}),
  Body: makeSlot({}),
  Backdrop: makeSlot(ABSOLUTE_FILL),
  AnimatePresence: PassThrough,
});

/**
 * Collapses app #1's eleven-part compound AlertDialog into one controlled
 * component. The feature only ever used it in that single confirm/cancel
 * shape, so the eleven exports were surface nobody spent.
 *
 * On gluestack rather than Tamagui: @tamagui/dialog forks 6 of its 16 files
 * for native and portals to `hostName: "root"` whenever `modal` is set, which
 * is the arrangement that made a route-hosted overlay swallow every touch.
 * gluestack ships zero native-side forks and keeps React Native's own `Modal`
 * strictly opt-in (`useRNModal` defaults false), so nothing presents a second
 * view controller.
 */
export function AlertDialog(props: AlertDialogProps) {
  const { open, onOpenChange } = props;
  const theme = useTheme();
  if (!open) return null;

  return (
    // The overlay host is mounted HERE, inside whatever route renders the
    // dialog -- not at the app root. That is the whole lesson of the
    // swallowed-touch bug: a host above the presented route is a host the
    // route's own view controller covers.
    <OverlayProvider>
      <GluestackAlertDialog
        isOpen={open}
        onClose={() => onOpenChange(false)}
        // Left at its default. Turning it on would present React Native's
        // Modal, i.e. a UIViewController on iOS, which is exactly the layer
        // this migration exists to stop stacking.
        useRNModal={false}
        closeOnOverlayClick
        isKeyboardDismissable
      >
        <GluestackAlertDialog.Backdrop style={styles.backdrop} />
        <View style={styles.centre}>
          {/* Themed, not the hardcoded white this used to carry: on the dark
              theme a white card is the one thing that is certainly wrong, and
              `background` would be the page's own colour. */}
          <GluestackAlertDialog.Content
            testID={props.testID}
            style={[
              styles.card,
              { backgroundColor: theme.color2?.val ?? '#fff' },
            ]}
          >
            {'children' in props ? props.children : <ConfirmBody {...props} />}
          </GluestackAlertDialog.Content>
        </View>
      </GluestackAlertDialog>
    </OverlayProvider>
  );
}

/** The question-and-two-buttons body of the prop-driven form. */
function ConfirmBody({
  onOpenChange,
  title,
  description,
  confirmLabel = 'Confirm',
  cancelLabel = 'Cancel',
  destructive = false,
  onConfirm,
  testID,
}: AlertDialogConfirmProps) {
  return (
    <>
      <YStack gap="$2">
        <Text variant="h4" testID={`${testID}-Title`}>
          {title}
        </Text>
        {description ? (
          <Text variant="muted" testID={`${testID}-Description`}>
            {description}
          </Text>
        ) : null}
      </YStack>
      <XStack gap="$3" justify="flex-end" mt="$3">
        <Button
          testID={`${testID}-Cancel`}
          chromeless
          onPress={() => onOpenChange(false)}
        >
          {cancelLabel}
        </Button>
        <Button
          testID={`${testID}-Confirm`}
          theme={destructive ? 'red' : 'blue'}
          bg="$color9"
          color="$color1"
          onPress={onConfirm}
        >
          {confirmLabel}
        </Button>
      </XStack>
    </>
  );
}

const styles = StyleSheet.create({
  backdrop: { backgroundColor: 'rgba(0,0,0,0.5)' },
  centre: {
    ...ABSOLUTE_FILL,
    alignItems: 'center',
    justifyContent: 'center',
    padding: 20,
    // as a style, not a prop: react-native-web warns on the prop, which fails the browser tests' zero-warnings gate
    pointerEvents: 'box-none',
  },
  card: {
    width: '90%',
    maxWidth: 420,
    borderRadius: 16,
    padding: 20,
    gap: 12,
  },
});

/**
 * Compound parts. Unlike the collapsed
 * `AlertDialog` above these are building blocks: they compose inside a
 * Tamagui `AlertDialog` root, exactly as `prompt-dialog.tsx` does.
 */
export type AlertDialogContentProps = YStackProps & {
  /**
   * Accepted for source compatibility. Tamagui's dialog portals only to the
   * PortalProvider root host (`hostName: "root"` is hardcoded inside
   * `@tamagui/dialog`), so a named host cannot be honored -- the value is
   * intentionally unused.
   */
  portalHost?: string;
  /**
   * Extra bottom offset (pixels) to lift the dialog above a keyboard
   * toolbar. Mirrors the design-system prop of the same name.
   */
  bottomOffset?: number;
};

/**
 * The composed body inside AlertDialog's gluestack card. It must not open a
 * Tamagui portal: no Tamagui dialog root is mounted, so that portal stayed closed and the card rendered empty.
 */
export function AlertDialogContent({
  portalHost: _portalHost,
  bottomOffset = 0,
  children,
  ...props
}: AlertDialogContentProps) {
  return (
    <YStack gap="$3" mb={bottomOffset} {...props}>
      {children}
    </YStack>
  );
}

export type AlertDialogHeaderProps = YStackProps;

export function AlertDialogHeader(props: AlertDialogHeaderProps) {
  return <YStack gap="$2" {...props} />;
}

export type AlertDialogFooterProps = XStackProps;

export function AlertDialogFooter(props: AlertDialogFooterProps) {
  return <XStack gap="$3" justify="flex-end" {...props} />;
}

export type AlertDialogTitleProps = TextProps;

export function AlertDialogTitle(props: AlertDialogTitleProps) {
  return <Text variant="h4" {...props} />;
}

export type AlertDialogDescriptionProps = TextProps;

export function AlertDialogDescription(props: AlertDialogDescriptionProps) {
  return <Text variant="muted" {...props} />;
}

export type AlertDialogActionProps = ButtonProps;

export function AlertDialogAction(props: AlertDialogActionProps) {
  return <Button theme="blue" bg="$color9" color="$color1" {...props} />;
}
