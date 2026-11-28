import { Platform } from 'react-native';

/**
 * What a long press opens, as props for a pressable view. On the web it is the view's context menu: there the view's own long press also
 * fired after a plain click that re-rendered it. A label makes it an action of the element, for a screen reader.
 */
export function longPress(open: (() => void) | undefined, label?: string) {
  if (!open) return {};
  if (Platform.OS === 'web')
    return {
      onContextMenu: (event: { preventDefault: () => void }) => {
        event.preventDefault();
        open();
      },
    };
  if (!label) return { onLongPress: open };
  return {
    onLongPress: open,
    accessibilityActions: [{ name: 'longpress', label }],
    onAccessibilityAction: (event: { nativeEvent: { actionName: string } }) => {
      if (event.nativeEvent.actionName === 'longpress') open();
    },
  };
}
