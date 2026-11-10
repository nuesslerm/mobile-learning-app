import { Input, Label, YStack, type InputProps } from '../primitives';

import { Text } from './text';

export type TextFieldProps = InputProps & {
  label?: string;
  error?: string;
  help?: string;
  testID: string;
};

/** Label + input + error/help, composed the way app #1's TextField is. */
export function TextField({
  label,
  error,
  help,
  testID,
  ...rest
}: TextFieldProps) {
  return (
    <YStack gap="$1.5" testID={`${testID}-Field`}>
      {label ? (
        <Label htmlFor={testID} size="$3">
          {label}
        </Label>
      ) : null}
      <Input
        id={testID}
        testID={testID}
        borderColor={error ? '$red8' : '$borderColor'}
        {...rest}
      />
      {error ? (
        <Text variant="small" color="$red10" testID={`${testID}-Error`}>
          {error}
        </Text>
      ) : help ? (
        <Text variant="muted" testID={`${testID}-Help`}>
          {help}
        </Text>
      ) : null}
    </YStack>
  );
}

export { Input, Label };
