'use strict';
// Draws what react-native-screens does with header items and a UIMenu as DOM, so a test sees the data a header declares:
// items, grouping flags, handlers, checked state, keep-open and drill-in. Never glass, the morph or UIKit's own layout.
const React = require('react');
const h = React.createElement;

const flag = (value) => (value === undefined ? undefined : String(value));
const itemName = (item) => item.accessibilityLabel ?? item.label;
const iconName = (icon) => (icon ? (icon.type === 'sfSymbol' ? icon.name : 'image') : undefined);
const row = { display: 'flex', alignItems: 'center', gap: 8 };
const column = { display: 'flex', flexDirection: 'column', alignItems: 'stretch' };

const itemData = (item) => ({
  'data-shares-background': flag(item.sharesBackground),
  'data-hides-shared-background': flag(item.hidesSharedBackground),
  'data-variant': item.variant,
  'data-identifier': item.identifier,
  'data-icon': iconName(item.icon),
});
const itemContent = (item) => [item.icon ? h('span', { key: 'icon', 'data-icon': iconName(item.icon), 'aria-hidden': 'true' }) : null, item.label];

const Rows = ({ items, onAction, onDrill }) =>
  items
    .filter((entry) => !entry.hidden)
    .map((entry, index) => {
      if (entry.type === 'submenu' && entry.inline) {
        return h('div', { key: index, role: 'group', 'aria-label': entry.label, 'data-testid': `NativeHeaderMenuSection-${entry.label}`, 'data-multiselectable': flag(entry.multiselectable), style: column }, h(Rows, { items: entry.items, onAction, onDrill }));
      }
      if (entry.type === 'submenu') {
        return h('button', { key: index, type: 'button', role: 'menuitem', 'aria-haspopup': 'menu', 'data-testid': `NativeHeaderMenuItem-${entry.label}`, 'data-destructive': flag(entry.destructive), 'data-multiselectable': flag(entry.multiselectable), 'data-icon': iconName(entry.icon), onClick: () => onDrill(entry.label) }, entry.label);
      }
      const checkable = entry.state !== undefined;
      return h(
        'button',
        {
          key: index,
          type: 'button',
          role: checkable ? 'menuitemcheckbox' : 'menuitem',
          'aria-checked': checkable ? (entry.state === 'mixed' ? 'mixed' : String(entry.state === 'on')) : undefined,
          disabled: entry.disabled === true,
          'data-testid': `NativeHeaderMenuItem-${entry.label}`,
          'data-destructive': flag(entry.destructive),
          'data-keeps-menu-presented': flag(entry.keepsMenuPresented),
          'data-icon': iconName(entry.icon),
          onClick: () => onAction(entry),
        },
        entry.label,
        entry.description ? h('span', { 'data-testid': `NativeHeaderMenuDescription-${entry.label}` }, entry.description) : null
      );
    });

const findSubmenu = (items, label) => {
  for (const entry of items) {
    if (entry.type !== 'submenu') continue;
    if (entry.inline) {
      const inner = findSubmenu(entry.items, label);
      if (inner) return inner;
    } else if (entry.label === label) return entry;
  }
  return undefined;
};

// Expo-router documents that a kept-open action recreates the menu, which closes every open submenu: the path resets.
const MenuItem = ({ item }) => {
  const [open, setOpen] = React.useState(false);
  const [path, setPath] = React.useState([]);
  const name = itemName(item);
  const close = () => {
    setOpen(false);
    setPath([]);
  };
  let level = item.menu;
  const trail = [];
  for (const label of path) {
    const next = findSubmenu(level.items, label);
    if (!next) break;
    level = next;
    trail.push(label);
  }
  const onAction = (entry) => {
    entry.onPress();
    if (entry.keepsMenuPresented) setPath([]);
    else close();
  };
  return h(
    'div',
    { 'data-testid': `NativeHeaderMenuHost-${name}`, style: { position: 'relative' } },
    h('button', { type: 'button', 'aria-haspopup': 'menu', 'aria-expanded': String(open), 'aria-label': name, disabled: item.disabled === true, 'data-testid': `NativeHeaderItem-${name}`, ...itemData(item), onClick: () => (open ? close() : setOpen(true)) }, itemContent(item)),
    open
      ? h(
          'div',
          { role: 'menu', 'aria-label': name, 'data-testid': `NativeHeaderMenu-${name}`, 'data-multiselectable': flag(level.multiselectable), style: { ...column, position: 'absolute', top: '100%', right: 0, zIndex: 1, minWidth: 200 } },
          trail.length > 0 ? h('button', { type: 'button', role: 'menuitem', 'data-testid': `NativeHeaderMenuHeader-${trail[trail.length - 1]}`, onClick: () => setPath(trail.slice(0, -1)) }, trail[trail.length - 1]) : level.title ? h('div', { 'data-testid': 'NativeHeaderMenuTitle' }, level.title) : null,
          h(Rows, { items: level.items, onAction, onDrill: (label) => setPath([...trail, label]) })
        )
      : null
  );
};

const Item = ({ item }) => {
  if (item.type === 'spacing') return h('span', { 'data-testid': 'NativeHeaderSpacing', 'data-spacing': String(item.spacing), style: { display: 'inline-block', width: item.spacing } });
  if (item.type === 'custom') return h('div', { 'data-testid': 'NativeHeaderCustom', 'data-hides-shared-background': flag(item.hidesSharedBackground) }, item.element);
  if (item.type === 'menu') return h(MenuItem, { item });
  return h('button', { type: 'button', 'aria-label': itemName(item), 'aria-pressed': item.selected === undefined ? undefined : String(item.selected), disabled: item.disabled === true, 'data-testid': `NativeHeaderItem-${itemName(item)}`, ...itemData(item), onClick: item.onPress }, itemContent(item));
};

const renderTitle = (options) => {
  const title = typeof options.title === 'string' ? options.title : '';
  if (typeof options.headerTitle === 'function') return options.headerTitle({ children: title, tintColor: options.headerTintColor });
  return typeof options.headerTitle === 'string' ? options.headerTitle : null;
};

// The bar a screen's options make native: only items or a headerTitle draw one; a bare `title` is the stock bar, not modelled.
const renderNativeHeader = (options) => {
  const left = options.unstable_headerLeftItems;
  const right = options.unstable_headerRightItems;
  const title = renderTitle(options);
  if (!left && !right && title === null) return null;
  const props = { tintColor: options.headerTintColor, canGoBack: true, backgroundColor: options.headerStyle?.backgroundColor };
  const items = (make) => (make ? make(props) : []).map((item, index) => h(Item, { key: index, item }));
  return h(
    'div',
    { 'data-testid': 'NativeHeader', 'data-header-shown': 'true', 'data-header-transparent': String(options.headerTransparent === true), style: { ...row, justifyContent: 'space-between', padding: 8 } },
    h('div', { 'data-testid': 'NativeHeaderLeft', role: 'group', style: row }, items(left)),
    title === null ? null : h('div', { 'data-testid': 'NativeHeaderTitle' }, title),
    h('div', { 'data-testid': 'NativeHeaderRight', role: 'group', style: row }, items(right))
  );
};

module.exports = { __esModule: true, renderNativeHeader };
