import type { HeaderMenuSpec, MenuNode } from './header-menu-spec';
import {
  isMenuSpec,
  type HeaderActionSpec,
  type HeaderGroupItem,
  type ScreenHeaderSpec,
} from './header-spec';

export type HandlerCall = (key: string) => void;

const BACK_KEY = 'back';
const pressKey = (testID: string) => `press:${testID}`;
const confirmKey = (testID: string) => `confirm:${testID}`;

function stableNode(node: MenuNode, call: HandlerCall): MenuNode {
  switch (node.kind) {
    case 'section':
    case 'submenu':
      return {
        ...node,
        items: node.items.map((item) => stableNode(item, call)),
      };
    case 'confirm':
      return { ...node, onConfirm: () => call(confirmKey(node.testID)) };
    default:
      return { ...node, onPress: () => call(pressKey(node.testID)) };
  }
}

function stableMenu(menu: HeaderMenuSpec, call: HandlerCall): HeaderMenuSpec {
  return { ...menu, items: menu.items.map((item) => stableNode(item, call)) };
}

function stableAction(
  action: HeaderActionSpec,
  call: HandlerCall
): HeaderActionSpec {
  return { ...action, onPress: () => call(pressKey(action.testID)) };
}

function stableMember(
  member: HeaderGroupItem,
  call: HandlerCall
): HeaderGroupItem {
  return isMenuSpec(member)
    ? stableMenu(member, call)
    : stableAction(member, call);
}

/** The spec with every handler swapped for one that hands its key to `call`, so a native bar built from it can outlive the closures of one render. */
export function withStableHandlers(
  spec: ScreenHeaderSpec,
  call: HandlerCall
): ScreenHeaderSpec {
  return {
    ...spec,
    onBack: spec.onBack ? () => call(BACK_KEY) : undefined,
    leadingStatus: spec.leadingStatus?.menu ? {
      ...spec.leadingStatus,
      menu: { ...spec.leadingStatus.menu, items: spec.leadingStatus.menu.items.map((item) => stableNode(item, call)) },
    } : spec.leadingStatus,
    actions: spec.actions?.map((action) => stableAction(action, call)),
    menu: spec.menu ? stableMenu(spec.menu, call) : undefined,
    groups: spec.groups?.map((group) =>
      group.map((member) => stableMember(member, call))
    ),
  };
}

function collectNodes(
  nodes: readonly MenuNode[],
  into: Map<string, () => void>
) {
  for (const node of nodes) {
    if (node.kind === 'section' || node.kind === 'submenu')
      collectNodes(node.items, into);
    else if (node.kind === 'confirm')
      into.set(confirmKey(node.testID), node.onConfirm);
    else into.set(pressKey(node.testID), node.onPress);
  }
}

/** The handlers of a spec under the keys `withStableHandlers` calls them by. */
export function collectHandlers(
  spec: ScreenHeaderSpec
): Map<string, () => void> {
  const handlers = new Map<string, () => void>();
  if (spec.onBack) handlers.set(BACK_KEY, spec.onBack);
  if (spec.leadingStatus?.menu) collectNodes(spec.leadingStatus.menu.items, handlers);
  const members: HeaderGroupItem[] = [
    ...(spec.actions ?? []),
    ...(spec.menu ? [spec.menu] : []),
    ...(spec.groups ?? []).flat(),
  ];
  for (const member of members) {
    if (isMenuSpec(member)) collectNodes(member.items, handlers);
    else handlers.set(pressKey(member.testID), member.onPress);
  }
  return handlers;
}
