import type {
  MenuConfirmNode,
  MenuNode,
  MenuSubmenuNode,
} from './header-menu-spec';

/** The ids of the submenus drilled into, outermost first; empty is the root list. */
export type MenuPath = readonly string[];

export type MenuNavigationAction =
  /** A submenu or confirm row was pressed. */
  | { type: 'open'; id: string }
  /** The header row of a submenu was pressed. */
  | { type: 'back' }
  /** A row inside a submenu was picked: the menu is rebuilt, as UIKit does, and shows the root again. */
  | { type: 'pick' }
  | { type: 'close' };

export const ROOT_PATH: MenuPath = [];

export function menuNavigation(
  path: MenuPath,
  action: MenuNavigationAction
): MenuPath {
  switch (action.type) {
    case 'open':
      return [...path, action.id];
    case 'back':
      return path.slice(0, -1);
    case 'pick':
    case 'close':
      return ROOT_PATH;
  }
}

export type MenuView =
  | { kind: 'list'; items: readonly MenuNode[]; header?: MenuSubmenuNode }
  | { kind: 'confirm'; node: MenuConfirmNode };

function find(items: readonly MenuNode[], id: string): MenuNode | undefined {
  for (const node of items) {
    if (node.testID === id) return node;
    if (node.kind === 'section') {
      const inner = find(node.items, id);
      if (inner) return inner;
    }
  }
  return undefined;
}

/** What the popover shows at a path; a path through a row that is gone shows the root again. */
export function resolveMenuView(
  items: readonly MenuNode[],
  path: MenuPath
): MenuView {
  let view: MenuView = { kind: 'list', items };
  for (const id of path) {
    if (view.kind !== 'list') return { kind: 'list', items };
    const node = find(view.items, id);
    if (node?.kind === 'submenu')
      view = { kind: 'list', items: node.items, header: node };
    else if (node?.kind === 'confirm') view = { kind: 'confirm', node };
    else return { kind: 'list', items };
  }
  return view;
}
