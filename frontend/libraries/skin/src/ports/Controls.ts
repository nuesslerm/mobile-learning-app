export interface ButtonProps {
  label: string;
  onPress: () => void;
  /** `danger` marks an action that discards or deletes. */
  tone?: 'default' | 'danger';
  disabled?: boolean;
  testID?: string;
}

export interface SpinnerProps {
  accessibilityLabel?: string;
}

export interface SwitchProps {
  value: boolean;
  onValueChange: (value: boolean) => void;
  /** Read aloud, since a switch has no text of its own. */
  accessibilityLabel: string;
  disabled?: boolean;
  testID?: string;
}

export interface TextFieldProps {
  value: string;
  onChangeText: (value: string) => void;
  placeholder?: string;
  accessibilityLabel?: string;
  keyboard?: 'default' | 'url';
  autoFocus?: boolean;
  /** The keyboard's return key was pressed. */
  onSubmit?: () => void;
  testID?: string;
}

/** A card that says something is wrong and offers the one way out. */
export interface NoticeProps {
  title: string;
  detail: string;
  actionLabel: string;
  onAction: () => void;
}
