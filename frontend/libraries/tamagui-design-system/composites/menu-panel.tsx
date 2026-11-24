import { GlassView, isGlassEffectAPIAvailable, isLiquidGlassAvailable } from 'expo-glass-effect';
import type { LucideIcon } from 'lucide-react-native';
import { useRef, useState, type ComponentRef, type ReactNode } from 'react';
import {
  Modal,
  Platform,
  Pressable,
  StyleSheet,
  Text as RNText,
  useWindowDimensions,
  View,
  type StyleProp,
  type ViewStyle,
} from 'react-native';

import { Check, ChevronDown, ChevronRight } from '../icons';
import { useTheme } from '../primitives';
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

export type MenuPanelProps = {
  icon: LucideIcon;
  accessibilityLabel: string;
  items: MenuNode[];
  /** Matches the control size of the sibling actions in the header pill. */
  size: number;
  testID: string;
  trigger?: ReactNode;
  triggerStyle?: StyleProp<ViewStyle>;
  placement?: 'above' | 'below' | 'beside';
  glass?: boolean;
  openPath?: MenuPath;
  panelWidth?: number;
};

const PANEL_WIDTH = 288;
const EDGE = 8;
const GAP = 6;
const FALLBACK_TOP = 96;

type Colors = {
  surface: string;
  border: string;
  label: string;
  muted: string;
  pressed: string;
  destructive: string;
  scrim: string;
};

type Columns = { checks: boolean; icons: boolean };
type Anchor = { x: number; y: number; width: number; height: number };

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

type RowProps = {
  testID: string;
  label: string;
  detail?: string;
  icon?: LucideIcon;
  checked?: boolean;
  destructive?: boolean;
  disabled?: boolean;
  trailing?: LucideIcon;
  bold?: boolean;
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
  bold,
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
            bold && styles.rowLabelBold,
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
      {node.message ? <RNText style={[styles.confirmMessage, { color: colors.muted }]}>{node.message}</RNText> : null}
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
 * The editor-style menu behind a header control on Android and web, drawn by the app because those platforms have no system pull-down menu.
 * It reads the same tree the iOS header turns into a UIMenu: a toggle keeps the panel open and flips its checkmark, a submenu replaces the list
 * in place under a header row that goes back, a pick inside a submenu returns to the root, and a confirm row becomes a one-button card.
 * A modal card under the trigger, mounted only while open (a portal cannot render on the server), so an outside tap closes it and nothing depends on a popover library's collision logic inside a header.
 */
export function MenuPanel({
  icon,
  accessibilityLabel,
  items,
  size,
  testID,
  trigger,
  triggerStyle,
  placement = 'below',
  glass = false,
  openPath = ROOT_PATH,
  panelWidth = PANEL_WIDTH,
}: MenuPanelProps) {
  const [open, setOpen] = useState(false);
  const [path, setPath] = useState<MenuPath>(ROOT_PATH);
  const [anchor, setAnchor] = useState<Anchor | null>(null);
  const triggerRef = useRef<ComponentRef<typeof Pressable>>(null);
  const { width: windowWidth, height: windowHeight } = useWindowDimensions();
  const theme = useTheme();

  const colors: Colors = {
    surface: theme.color2?.val ?? '#1c1c1e',
    border: theme.borderColor?.val ?? 'rgba(128,128,128,0.35)',
    label: theme.color?.val ?? '#ffffff',
    muted: theme.color11?.val ?? 'rgba(160,160,160,1)',
    pressed: theme.color5?.val ?? 'rgba(128,128,128,0.2)',
    destructive: theme.destructive?.val ?? '#ff453a',
    scrim: 'rgba(0,0,0,0.28)',
  };

  const view = resolveMenuView(items, path);

  function go(action: MenuNavigationAction) {
    setPath((current) => menuNavigation(current, action));
  }

  function openPanel() {
    triggerRef.current?.measureInWindow((x, y, width, height) => {
      setAnchor(width > 0 ? { x, y, width, height } : null);
    });
    setPath(openPath);
    setOpen(true);
  }

  function closePanel() {
    setOpen(false);
    go({ type: 'close' });
  }

  function pressAction(node: MenuActionNode) {
    if (node.keepOpen) {
      node.onPress();
      go({ type: 'pick' });
      return;
    }
    // Close before acting: several of these navigate, and a panel still mounted over a replaced screen floats above the wrong route.
    closePanel();
    node.onPress();
  }

  function renderList(
    nodes: readonly MenuNode[],
    columns: Columns
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
            {renderList(node.items, columns)}
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
            closePanel();
            view.node.onConfirm();
          }}
        />
      );
    }
    const list = renderList(view.items, columnsOf(view.items));
    if (!view.header) return <View style={styles.list}>{list}</View>;
    return (
      <View style={styles.list}>
        <MenuRow
          testID={`${view.header.testID}Header`}
          label={view.header.label}
          icon={view.header.icon}
          trailing={ChevronDown}
          bold
          columns={{ checks: false, icons: view.header.icon !== undefined }}
          colors={colors}
          onPress={() => go({ type: 'back' })}
        />
        <View style={[styles.divider, { backgroundColor: colors.border }]} />
        {list}
      </View>
    );
  }

  const width = Math.min(panelWidth, windowWidth - 2 * EDGE);
  const beside = placement === 'beside' && anchor && anchor.x + anchor.width + GAP * 2 + width + EDGE <= windowWidth;
  const nativeGlass = glass && Platform.OS === 'ios' && isLiquidGlassAvailable() && isGlassEffectAPIAvailable();
  const right = anchor
    ? Math.min(windowWidth - width - EDGE, Math.max(EDGE, windowWidth - (anchor.x + anchor.width)))
    : EDGE + 4;
  const position = beside
    ? { left: anchor.x + anchor.width + GAP * 2, bottom: Math.max(EDGE, windowHeight - anchor.y - anchor.height - 20) }
    : {
        right,
        ...(placement === 'below'
          ? { top: anchor ? anchor.y + anchor.height + GAP : FALLBACK_TOP }
          : { bottom: anchor ? Math.max(EDGE, windowHeight - anchor.y + GAP) : FALLBACK_TOP }),
      };

  return (
    <>
      <Pressable
        ref={triggerRef}
        testID={testID}
        accessibilityRole="button"
        aria-label={accessibilityLabel}
        aria-expanded={open}
        onPress={openPanel}
        style={({ pressed: isPressed }) => [
          styles.trigger,
          trigger ? { minWidth: size, height: size, paddingHorizontal: 4 } : { width: size, height: size },
          triggerStyle,
          isPressed && { backgroundColor: colors.pressed },
        ]}
      >
        {trigger ?? <Icon as={icon} size={19} />}
      </Pressable>
      {open ? (
        <Modal
          visible
          transparent
          animationType="none"
          onRequestClose={closePanel}
          statusBarTranslucent
        >
          <Pressable
            testID={`${testID}Backdrop`}
            accessibilityLabel="Close menu"
            onPress={closePanel}
            style={[styles.backdrop, { backgroundColor: nativeGlass ? "transparent" : colors.scrim }]}
          />
          <View
            testID={`${testID}Content`}
            style={[
              styles.card,
              {
                width,
                ...position,
                backgroundColor: nativeGlass ? 'transparent' : colors.surface,
                borderColor: colors.border,
              },
            ]}
          >
            {nativeGlass ? (
              <GlassView pointerEvents="none" glassEffectStyle="regular" tintColor="rgba(20,20,22,0.45)" style={styles.glassCard} />
            ) : null}
            {renderView()}
          </View>
        </Modal>
      ) : null}
    </>
  );
}

const styles = StyleSheet.create({
  trigger: {
    alignItems: 'center',
    justifyContent: 'center',
    borderRadius: 9999,
  },
  backdrop: StyleSheet.absoluteFill,
  card: {
    position: 'absolute',
    borderRadius: 20,
    borderWidth: StyleSheet.hairlineWidth,
    overflow: 'hidden',
    shadowColor: '#000',
    shadowOpacity: 0.28,
    shadowRadius: 24,
    shadowOffset: { width: 0, height: 10 },
    elevation: 16,
  },
  glassCard: { ...StyleSheet.absoluteFill, borderRadius: 20 },
  list: { paddingVertical: 6 },
  divider: { height: StyleSheet.hairlineWidth, marginVertical: 4 },
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingHorizontal: 16,
    paddingVertical: 11,
    minHeight: 48,
  },
  rowDisabled: { opacity: 0.4 },
  rowCheck: { width: 26, alignItems: 'flex-start', justifyContent: 'center' },
  rowIcon: { width: 32, alignItems: 'flex-start', justifyContent: 'center' },
  // minWidth:0 lets numberOfLines truncate instead of stretching the row past the card.
  rowText: { flex: 1, minWidth: 0 },
  rowTrailing: {
    marginLeft: 8,
    alignItems: 'flex-end',
    justifyContent: 'center',
  },
  rowLabel: { fontSize: 17, fontWeight: '400' },
  rowLabelBold: { fontWeight: '600' },
  rowDetail: { fontSize: 13, marginTop: 1 },
  section: { paddingTop: 4, marginTop: 4 },
  sectionTitle: { fontSize: 13, paddingHorizontal: 16, paddingVertical: 6 },
  confirm: { padding: 20, gap: 6 },
  confirmTitle: { fontSize: 17, fontWeight: '600' },
  confirmMessage: { fontSize: 15, marginBottom: 12 },
  confirmButton: {
    minHeight: 50,
    borderRadius: 25,
    alignItems: 'center',
    justifyContent: 'center',
  },
  confirmLabel: { fontSize: 17, fontWeight: '600' },
});
