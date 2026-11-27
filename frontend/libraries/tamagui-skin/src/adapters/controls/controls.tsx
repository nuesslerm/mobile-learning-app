import type { ButtonProps, NoticeProps, SpinnerProps, SwitchProps, TextFieldProps } from '@lib/skin';
import {
  Admonition,
  Button as DesignButton,
  Spinner as DesignSpinner,
  Switch as DesignSwitch,
  TextField as DesignTextField,
} from '@lib/tamagui-design-system';

export function Button(props: ButtonProps) {
  return (
    <DesignButton
      variant={props.tone === 'danger' ? 'destructive' : 'outline'}
      // the app's themes have no red sub-theme, so the destructive variant's own fill resolves to the accent
      {...(props.tone === 'danger' ? { bg: '$red9' } : {})}
      size="$3"
      disabled={props.disabled}
      onPress={props.onPress}
      testID={props.testID}
    >
      {props.label}
    </DesignButton>
  );
}

export function Spinner(props: SpinnerProps) {
  return <DesignSpinner accessibilityLabel={props.accessibilityLabel} />;
}

export function Switch(props: SwitchProps) {
  return (
    <DesignSwitch
      value={props.value}
      onValueChange={props.onValueChange}
      accessibilityLabel={props.accessibilityLabel}
      disabled={props.disabled}
      testID={props.testID ?? 'skin-switch'}
    />
  );
}

export function TextField(props: TextFieldProps) {
  return (
    <DesignTextField
      value={props.value}
      onChangeText={props.onChangeText}
      placeholder={props.placeholder}
      placeholderTextColor="$color10"
      aria-label={props.accessibilityLabel ?? props.placeholder}
      autoFocus={props.autoFocus}
      autoCapitalize="none"
      autoCorrect={false}
      keyboardType={props.keyboard === 'url' ? 'url' : 'default'}
      onSubmitEditing={props.onSubmit}
      testID={props.testID ?? 'skin-text-field'}
    />
  );
}

export function Notice(props: NoticeProps) {
  return (
    <Admonition variant="warning" testID="skin-notice" mx="$3" my="$2.5" role="alert" aria-live="polite">
      <Admonition.Title>{props.title}</Admonition.Title>
      <Admonition.Body>{props.detail}</Admonition.Body>
      <Admonition.Action>
        <DesignButton variant="outline" size="$3" self="flex-start" onPress={props.onAction}>
          {props.actionLabel}
        </DesignButton>
      </Admonition.Action>
    </Admonition>
  );
}
