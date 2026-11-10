import type { LucideIcon } from 'lucide-react-native';
import { useState } from 'react';
import { Pressable, StyleSheet, Text as RNText, View } from 'react-native';

import { Popover, useTheme } from '../primitives';
import { Icon } from '../ui/icon';

export type HeaderMenuItem = {
  icon: LucideIcon;
  label: string;
  /** Second line under the label -- context, not restatement. */
  detail?: string;
  onPress: () => void;
  disabled?: boolean;
  testID: string;
};

export type HeaderMenuProps = {
  icon: LucideIcon;
  accessibilityLabel: string;
  items: HeaderMenuItem[];
  /** Matches the control size of the sibling actions in the header pill. */
  size: number;
  testID: string;
};

const MENU_WIDTH = 260;

/**
 * The overflow menu behind a header's "..." control.
 *
 * An ANCHORED dropdown on every platform. Tamagui's docs suggest swapping to a
 * Sheet on mobile via `Adapt`, and that was tried and reverted: a bottom sheet
 * is not what a header overflow menu is, and it is further from the reference
 * (Claude Code's own header menu, which is anchored) than this.
 *
 * Tamagui's `Menu` -- the ready-made native one -- is not an option here:
 * `@tamagui/create-menu` returns `null` on native unless an adapter is
 * registered, and every adapter (zeego, @expo/ui) needs native modules and so a
 * dev client. Expo Go on SDK 54 cannot have it. See docs/known-gaps.md.
 *
 * The ROWS are plain React Native primitives with explicit StyleSheet values,
 * not Tamagui shorthands. That is a deliberate retreat: the first version used
 * `px`/`py`/`gap`/`items` shorthands and a `<Text variant>` from this design
 * system, and on a real device it rendered three bare icons floating in a box
 * four times too tall -- no labels at all -- while looking correct in every
 * browser check. Popover itself is kept (its anchoring is what this needs, and
 * it resolves to Popover.native.js in the iOS bundle); only the layout inside
 * it is expressed in primitives whose native behaviour is not in question.
 *
 * Colours still come from the theme, via `useTheme()` rather than tokens, so
 * dark mode works without relying on token resolution inside a portal.
 */
export function HeaderMenu({
  icon,
  accessibilityLabel,
  items,
  size,
  testID,
}: HeaderMenuProps) {
  const [open, setOpen] = useState(false);
  const theme = useTheme();

  const surface = theme.color2?.val ?? '#1c1c1e';
  const border = theme.borderColor?.val ?? 'rgba(128,128,128,0.35)';
  const label = theme.color?.val ?? '#ffffff';
  const muted = theme.color11?.val ?? 'rgba(160,160,160,1)';
  const pressed = theme.color5?.val ?? 'rgba(128,128,128,0.2)';

  return (
    // No `allowFlip`. floating-ui's collision boundary on native is the
    // reference's offsetParent, and this trigger lives inside the native-stack
    // header's own short view -- so "room below" measures as ~40pt, flip fires,
    // and the card renders upward off the top of the screen with only its last
    // row visible under the status bar. Confirmed on a device. A header
    // overflow button is always at the top with the whole screen beneath it, so
    // there is nothing for flip to decide; web never showed it because there
    // the boundary is the window.
    <Popover open={open} onOpenChange={setOpen} placement="bottom-end">
      {/* Pressable, not View: `asChild` clones the child with Popover's own
          onPress, and a plain RN View has nowhere to put it -- the trigger
          silently stopped opening the menu. */}
      <Popover.Trigger asChild>
        <Pressable
          testID={testID}
          accessibilityRole="button"
          aria-label={accessibilityLabel}
          style={({ pressed: isPressed }) => [
            styles.trigger,
            { width: size, height: size },
            isPressed && { backgroundColor: pressed },
          ]}
        >
          <Icon as={icon} size={19} />
        </Pressable>
      </Popover.Trigger>

      {/* `unstyled` rather than transparent overrides: Popover.Content is a
          styled frame in its own right, and painting a card inside a
          background-and-border it still renders is what produced the doubled
          black outline around the menu. */}
      <Popover.Content
        testID={`${testID}Content`}
        unstyled
        width={MENU_WIDTH}
        enterStyle={{ opacity: 0, y: -6 }}
        exitStyle={{ opacity: 0, y: -6 }}
      >
        <View
          style={[
            styles.card,
            { backgroundColor: surface, borderColor: border },
          ]}
        >
          {items.map((item, index) => (
            <Pressable
              key={item.testID}
              testID={item.testID}
              accessibilityRole="button"
              aria-label={item.label}
              accessibilityState={{ disabled: item.disabled }}
              disabled={item.disabled}
              onPress={() => {
                // Close before acting: several of these navigate, and a popover
                // still mounted over a screen that has already been replaced is
                // how you get a menu floating above the wrong route.
                setOpen(false);
                item.onPress();
              }}
              style={({ pressed: isPressed }) => [
                styles.row,
                index > 0 && {
                  borderTopWidth: StyleSheet.hairlineWidth,
                  borderTopColor: border,
                },
                item.disabled && styles.rowDisabled,
                isPressed && !item.disabled && { backgroundColor: pressed },
              ]}
            >
              <View style={styles.rowIcon}>
                <Icon as={item.icon} size={19} />
              </View>
              <View style={styles.rowText}>
                <RNText
                  numberOfLines={1}
                  style={[styles.rowLabel, { color: label }]}
                >
                  {item.label}
                </RNText>
                {item.detail ? (
                  <RNText
                    numberOfLines={1}
                    style={[styles.rowDetail, { color: muted }]}
                  >
                    {item.detail}
                  </RNText>
                ) : null}
              </View>
            </Pressable>
          ))}
        </View>
      </Popover.Content>
    </Popover>
  );
}

const styles = StyleSheet.create({
  trigger: {
    alignItems: 'center',
    justifyContent: 'center',
    borderRadius: 9999,
  },
  card: {
    width: '100%',
    borderRadius: 14,
    borderWidth: StyleSheet.hairlineWidth,
    overflow: 'hidden',
  },
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingHorizontal: 14,
    paddingVertical: 12,
    minHeight: 48,
  },
  rowDisabled: { opacity: 0.4 },
  rowIcon: { width: 28, alignItems: 'flex-start', justifyContent: 'center' },
  // minWidth:0 is what lets numberOfLines truncate instead of forcing the row
  // wider than the card -- without it a long detail line stretches the layout.
  rowText: { flex: 1, minWidth: 0 },
  rowLabel: { fontSize: 16, fontWeight: '500' },
  rowDetail: { fontSize: 13, marginTop: 1 },
});
