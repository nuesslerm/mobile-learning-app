import type { NativeStackHeaderItemButton } from 'expo-router';
import type { LucideIcon } from 'lucide-react-native';

// Typed from expo-router's public item type, so a misspelt SF Symbol name fails the typecheck.
export type SfSymbolName = Extract<
  NonNullable<NativeStackHeaderItemButton['icon']>,
  { type: 'sfSymbol' }
>['name'];

type MenuNodeBase = {
  /** Becomes the row's testID and its key: unique in the whole tree. */
  testID: string;
  icon?: LucideIcon;
  sfSymbol?: SfSymbolName;
};

/** A row that runs a handler. A toggle or an option sets `checked`; `keepOpen` leaves the menu showing after the press. */
export type MenuActionNode = MenuNodeBase & {
  /** Absent on today's HeaderMenuItem objects. */
  kind?: 'action';
  label: string;
  /** Second line under the label. */
  detail?: string;
  /** Inline PNGs for SwiftUI menus. Their pixel dimensions are used as points. */
  iosMenuImage?: {
    light: string;
    dark: string;
  };
  checked?: boolean;
  keepOpen?: boolean;
  afterDismiss?: boolean;
  destructive?: boolean;
  disabled?: boolean;
  onPress: () => void;
};

/** Rows set apart by a hairline, with an optional heading. */
export type MenuSectionNode = {
  kind: 'section';
  testID: string;
  title?: string;
  items: MenuNode[];
};

/** A row that opens its own list in place of the current one. */
export type MenuSubmenuNode = MenuNodeBase & {
  kind: 'submenu';
  label: string;
  destructive?: boolean;
  items: MenuNode[];
};

/** A row that opens a one-button confirmation: `title` and `message` explain, `confirmLabel` runs `onConfirm`. */
export type MenuConfirmNode = MenuNodeBase & {
  kind: 'confirm';
  label: string;
  title: string;
  message: string;
  confirmLabel: string;
  onConfirm: () => void;
};

export type MenuNode =
  | MenuActionNode
  | MenuSectionNode
  | MenuSubmenuNode
  | MenuConfirmNode;

/** The control that opens a menu from a header: its glyph, its name and the tree it shows. */
export type HeaderMenuSpec = {
  testID: string;
  accessibilityLabel: string;
  /** Text shown beside the glyph on the native bar (UIKit decides whether it shows); the accessibility label when absent. */
  label?: string;
  icon: LucideIcon;
  sfSymbol?: SfSymbolName;
  /** Heading on top of the native menu. */
  title?: string;
  items: MenuNode[];
};

export function isActionNode(node: MenuNode): node is MenuActionNode {
  return node.kind === undefined || node.kind === 'action';
}

export type MenuTreeValidation = { valid: boolean; duplicateIds: string[] };

export function validateMenuTree(
  items: readonly MenuNode[]
): MenuTreeValidation {
  const seen = new Set<string>();
  const duplicateIds: string[] = [];
  const visit = (nodes: readonly MenuNode[]) => {
    for (const node of nodes) {
      if (seen.has(node.testID) && !duplicateIds.includes(node.testID))
        duplicateIds.push(node.testID);
      seen.add(node.testID);
      if (node.kind === 'section' || node.kind === 'submenu') visit(node.items);
    }
  };
  visit(items);
  return { valid: duplicateIds.length === 0, duplicateIds };
}

function project(node: MenuNode): unknown {
  switch (node.kind) {
    case 'section':
      return {
        k: 'section',
        id: node.testID,
        title: node.title,
        items: node.items.map(project),
      };
    case 'submenu':
      return {
        k: 'submenu',
        id: node.testID,
        label: node.label,
        sf: node.sfSymbol,
        destructive: node.destructive,
        items: node.items.map(project),
      };
    case 'confirm':
      return {
        k: 'confirm',
        id: node.testID,
        label: node.label,
        sf: node.sfSymbol,
        title: node.title,
        message: node.message,
        confirm: node.confirmLabel,
      };
    default:
      return {
        k: 'action',
        id: node.testID,
        label: node.label,
        detail: node.detail,
        sf: node.sfSymbol,
        iosMenuImage: node.iosMenuImage,
        checked: node.checked,
        keepOpen: node.keepOpen,
        destructive: node.destructive,
        disabled: node.disabled,
      };
  }
}

/** What a menu shows, without its handlers: equal for two trees that differ only in closure identity, so a native menu is rebuilt only when it displays something new. */
export function menuSignature(items: readonly MenuNode[]): string {
  return JSON.stringify(items.map(project));
}
