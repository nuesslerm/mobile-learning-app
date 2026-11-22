import type { LucideIcon } from 'lucide-react-native';
import { useState, type ReactNode } from 'react';
import { Pressable, StyleSheet, Text as RNText, View } from 'react-native';

import { Check, ChevronDown, ChevronRight } from '../icons';
import { Popover, useTheme } from '../primitives';
import { Icon } from '../ui/icon';

import {
  menuNavigation,
  resolveMenuView,
  ROOT_PATH,
  type MenuNavigationAction,
  type MenuPath,
} from './native-header/header-menu-navigation';
import {
  isActionNode,
  type MenuActionNode,
  type MenuConfirmNode,
  type MenuNode,
} from './native-header/header-menu-spec';

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
  /** Rows as today (HeaderMenuItem objects have no `kind`), or the menu tree: toggles, sections, submenus, a confirmation. */
  items: MenuNode[];
  /** Matches the control size of the sibling actions in the header pill. */
  size: number;
  testID: string;
};

const MENU_WIDTH = 260;

type Colors = {
  surface: string;
  border: string;
  label: string;
  muted: string;
  pressed: string;
  destructive: string;
};

type Columns = { checks: boolean; icons: boolean };

function leaves(nodes: readonly MenuNode[]): MenuNode[] {
  return nodes.flatMap((node) =>
    node.kind === 'section' ? leaves(node.items) : [node]
  );
}

function columnsOf(nodes: readonly MenuNode[]): Columns {
  const rows = leaves(nodes);
  return {
    checks: rows.some((row) => isActionNode(row) && row.checked !== undefined),
    icons: rows.some((row) => row.kind !== 'section' && row.icon !== undefined),
  };
}

// A list of plain actions keeps today's hairline between rows; once a menu has toggles, sections or submenus only the sections are set apart, as in the reference.
function isPlainList(nodes: readonly MenuNode[]): boolean {
  return (
    leaves(nodes).length === nodes.length &&
    nodes.every((node) => isActionNode(node) && node.checked === undefined)
  );
}

type RowProps = {
  testID: string;
  label: string;
  detail?: string;
  icon?: LucideIcon;
  checked?: boolean;
  destructive?: boolean;
  disabled?: boolean;
  trailing?: LucideIcon;
  separated: boolean;
  columns: Columns;
  colors: Colors;
  onPress?: () => void;
};

function MenuRow({
  testID,
  label,
  detail,
  icon,
  checked,
  destructive,
  disabled,
  trailing,
  separated,
  columns,
  colors,
  onPress,
}: RowProps) {
  return (
    <Pressable
      testID={testID}
      accessibilityRole={checked === undefined ? 'button' : 'checkbox'}
      aria-label={label}
      aria-checked={checked}
      accessibilityState={{ disabled }}
      disabled={disabled}
      onPress={onPress}
      style={({ pressed: isPressed }) => [
        styles.row,
        separated && {
          borderTopWidth: StyleSheet.hairlineWidth,
          borderTopColor: colors.border,
        },
        disabled && styles.rowDisabled,
        isPressed && !disabled && { backgroundColor: colors.pressed },
      ]}
    >
      {columns.checks ? (
        <View testID={`${testID}Check`} style={styles.rowCheck}>
          {checked ? <Icon as={Check} size={17} /> : null}
        </View>
      ) : null}
      {columns.icons ? (
        <View style={styles.rowIcon}>
          {icon ? <Icon as={icon} size={19} /> : null}
        </View>
      ) : null}
      <View style={styles.rowText}>
        <RNText
          numberOfLines={1}
          style={[
            styles.rowLabel,
            { color: destructive ? colors.destructive : colors.label },
          ]}
        >
          {label}
        </RNText>
        {detail ? (
          <RNText
            numberOfLines={1}
            style={[styles.rowDetail, { color: colors.muted }]}
          >
            {detail}
          </RNText>
        ) : null}
      </View>
      {trailing ? (
        <View style={styles.rowTrailing}>
          <Icon as={trailing} size={17} color="$color11" />
        </View>
      ) : null}
    </Pressable>
  );
}

function ConfirmCard({
  node,
  colors,
  onConfirm,
}: {
  node: MenuConfirmNode;
  colors: Colors;
  onConfirm: () => void;
}) {
  return (
    <View testID={`${node.testID}Card`} style={styles.confirm}>
      <RNText style={[styles.confirmTitle, { color: colors.label }]}>
        {node.title}
      </RNText>
      <RNText style={[styles.confirmMessage, { color: colors.muted }]}>
        {node.message}
      </RNText>
      <Pressable
        testID={`${node.testID}Confirm`}
        accessibilityRole="button"
        aria-label={node.confirmLabel}
        onPress={onConfirm}
        style={({ pressed: isPressed }) => [
          styles.confirmButton,
          { backgroundColor: isPressed ? colors.pressed : colors.border },
        ]}
      >
        <RNText style={[styles.confirmLabel, { color: colors.destructive }]}>
          {node.confirmLabel}
        </RNText>
      </Pressable>
    </View>
  );
}

/**
 * The overflow menu behind a header's "..." control.
 *
 * An ANCHORED dropdown on every platform. Tamagui's docs suggest swapping to a
 * Sheet on mobile via `Adapt`, and that was tried and reverted: a bottom sheet
 * is not what a header overflow menu is, and it is further from the reference
 * (Claude Code's own header menu, which is anchored) than this.
 *
 * It is the menu on Android and on the web, and on iOS where the native glass
 * bar is off: the native bar draws the same tree as a UIMenu. A submenu
 * replaces the list in place under a header row that returns, a pick inside it
 * returns to the root (what a rebuilt UIMenu does), and a confirm row opens a
 * one-button card. Tamagui's `Menu` is not used because `@tamagui/create-menu`
 * returns `null` on native unless an adapter is registered, and every adapter
 * (zeego, @expo/ui) needs native modules; Stack.Toolbar renders nothing on
 * web, needs @expo/ui on Android and has no glass.
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
  const [path, setPath] = useState<MenuPath>(ROOT_PATH);
  const theme = useTheme();

  const colors: Colors = {
    surface: theme.color2?.val ?? '#1c1c1e',
    border: theme.borderColor?.val ?? 'rgba(128,128,128,0.35)',
    label: theme.color?.val ?? '#ffffff',
    muted: theme.color11?.val ?? 'rgba(160,160,160,1)',
    pressed: theme.color5?.val ?? 'rgba(128,128,128,0.2)',
    destructive: theme.destructive?.val ?? '#ff453a',
  };

  const view = resolveMenuView(items, path);

  function go(action: MenuNavigationAction) {
    setPath((current) => menuNavigation(current, action));
  }

  function closeMenu() {
    setOpen(false);
    go({ type: 'close' });
  }

  function pressAction(node: MenuActionNode) {
    if (node.keepOpen) {
      node.onPress();
      go({ type: 'pick' });
      return;
    }
    // Close before acting: several of these navigate, and a popover
    // still mounted over a screen that has already been replaced is
    // how you get a menu floating above the wrong route.
    closeMenu();
    node.onPress();
  }

  function renderList(
    nodes: readonly MenuNode[],
    columns: Columns,
    plain: boolean
  ): ReactNode[] {
    return nodes.map((node, index) => {
      if (node.kind === 'section') {
        return (
          <View
            key={node.testID}
            testID={node.testID}
            style={[
              styles.section,
              index > 0 && {
                borderTopWidth: StyleSheet.hairlineWidth,
                borderTopColor: colors.border,
              },
            ]}
          >
            {node.title ? (
              <RNText style={[styles.sectionTitle, { color: colors.muted }]}>
                {node.title}
              </RNText>
            ) : null}
            {renderList(node.items, columns, false)}
          </View>
        );
      }
      if (isActionNode(node)) {
        return (
          <MenuRow
            key={node.testID}
            testID={node.testID}
            label={node.label}
            detail={node.detail}
            icon={node.icon}
            checked={node.checked}
            destructive={node.destructive}
            disabled={node.disabled}
            separated={plain && index > 0}
            columns={columns}
            colors={colors}
            onPress={() => pressAction(node)}
          />
        );
      }
      return (
        <MenuRow
          key={node.testID}
          testID={node.testID}
          label={node.label}
          icon={node.icon}
          destructive={node.kind === 'submenu' ? node.destructive : undefined}
          trailing={ChevronRight}
          separated={false}
          columns={columns}
          colors={colors}
          onPress={() => go({ type: 'open', id: node.testID })}
        />
      );
    });
  }

  function renderView(): ReactNode {
    if (view.kind === 'confirm') {
      return (
        <ConfirmCard
          node={view.node}
          colors={colors}
          onConfirm={() => {
            closeMenu();
            view.node.onConfirm();
          }}
        />
      );
    }
    const list = renderList(
      view.items,
      columnsOf(view.items),
      isPlainList(view.items)
    );
    if (!view.header) return list;
    return (
      <>
        <View
          style={{
            borderBottomWidth: StyleSheet.hairlineWidth,
            borderBottomColor: colors.border,
          }}
        >
          <MenuRow
            testID={`${view.header.testID}Header`}
            label={view.header.label}
            icon={view.header.icon}
            trailing={ChevronDown}
            separated={false}
            columns={{ checks: false, icons: view.header.icon !== undefined }}
            colors={colors}
            onPress={() => go({ type: 'back' })}
          />
        </View>
        {list}
      </>
    );
  }

  return (
    // No `allowFlip`. floating-ui's collision boundary on native is the
    // reference's offsetParent, and this trigger lives inside the native-stack
    // header's own short view -- so "room below" measures as ~40pt, flip fires,
    // and the card renders upward off the top of the screen with only its last
    // row visible under the status bar. Confirmed on a device. A header
    // overflow button is always at the top with the whole screen beneath it, so
    // there is nothing for flip to decide; web never showed it because there
    // the boundary is the window.
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) go({ type: 'close' });
      }}
      placement="bottom-end"
    >
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
            isPressed && { backgroundColor: colors.pressed },
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
            { backgroundColor: colors.surface, borderColor: colors.border },
          ]}
        >
          {renderView()}
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
  rowCheck: { width: 24, alignItems: 'flex-start', justifyContent: 'center' },
  rowIcon: { width: 28, alignItems: 'flex-start', justifyContent: 'center' },
  // minWidth:0 is what lets numberOfLines truncate instead of forcing the row
  // wider than the card -- without it a long detail line stretches the layout.
  rowText: { flex: 1, minWidth: 0 },
  rowTrailing: {
    marginLeft: 8,
    alignItems: 'flex-end',
    justifyContent: 'center',
  },
  rowLabel: { fontSize: 16, fontWeight: '500' },
  rowDetail: { fontSize: 13, marginTop: 1 },
  section: { paddingVertical: 4 },
  sectionTitle: { fontSize: 13, paddingHorizontal: 14, paddingVertical: 6 },
  confirm: { padding: 16, gap: 6 },
  confirmTitle: { fontSize: 17, fontWeight: '600' },
  confirmMessage: { fontSize: 15, marginBottom: 10 },
  confirmButton: {
    minHeight: 48,
    borderRadius: 24,
    alignItems: 'center',
    justifyContent: 'center',
  },
  confirmLabel: { fontSize: 17, fontWeight: '600' },
});
