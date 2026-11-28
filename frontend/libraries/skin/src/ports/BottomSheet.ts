import type { ComponentRef, ReactNode, Ref } from 'react';
import type { StyleProp, View, ViewProps, ViewStyle } from 'react-native';

export interface BottomSheetProps {
  visible?: boolean;
  children: (dismiss: (after?: () => void) => void) => ReactNode;
  onClose: () => void;
  onBack?: () => void;
  onDismiss?: () => void;
  label?: string;
  style: StyleProp<ViewStyle>;
  onLayout?: ViewProps['onLayout'];
  panelRef?: Ref<ComponentRef<typeof View>>;
  avoidKeyboard?: boolean;
  keyboardInset?: number;
}
