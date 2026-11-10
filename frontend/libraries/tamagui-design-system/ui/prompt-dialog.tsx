import { useEffect, useState } from 'react';

import {
  AlertDialog as TamaguiAlertDialog,
  Button,
  Input,
  XStack,
  YStack,
} from '../primitives';

import { Text } from './text';

export type PromptDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: string;
  initialValue?: string;
  placeholder?: string;
  confirmLabel?: string;
  onSubmit: (value: string) => void;
  /** See `AlertDialog`'s `modal` -- false when a route is already the modal. */
  modal?: boolean;
  testID: string;
};

/** Controlled `Alert.prompt` equivalent -- an AlertDialog with a text input. */
export function PromptDialog({
  open,
  onOpenChange,
  title,
  description,
  initialValue = '',
  placeholder,
  confirmLabel = 'Save',
  onSubmit,
  modal = true,
  testID,
}: PromptDialogProps) {
  const [value, setValue] = useState(initialValue);

  // Reset on each open: a dialog reopened after a cancel must not still hold
  // the abandoned edit.
  useEffect(() => {
    if (open) setValue(initialValue);
  }, [open, initialValue]);

  return (
    <TamaguiAlertDialog open={open} onOpenChange={onOpenChange} modal={modal}>
      <TamaguiAlertDialog.Portal>
        <TamaguiAlertDialog.Overlay key="overlay" bg="$shadow6" opacity={0.5} />
        <TamaguiAlertDialog.Content
          key="content"
          testID={testID}
          bordered
          elevate
          gap="$3"
          width="90%"
        >
          <YStack gap="$2">
            <Text variant="h4" testID={`${testID}-Title`}>
              {title}
            </Text>
            {description ? (
              <Text variant="muted" testID={`${testID}-Description`}>
                {description}
              </Text>
            ) : null}
          </YStack>
          <Input
            testID={`${testID}-Input`}
            value={value}
            onChangeText={setValue}
            placeholder={placeholder}
            autoFocus
            onSubmitEditing={() => onSubmit(value)}
          />
          <XStack gap="$3" justify="flex-end">
            <TamaguiAlertDialog.Cancel asChild>
              <Button testID={`${testID}-Cancel`} chromeless>
                Cancel
              </Button>
            </TamaguiAlertDialog.Cancel>
            <TamaguiAlertDialog.Action asChild>
              <Button
                testID={`${testID}-Confirm`}
                theme="blue"
                bg="$color9"
                color="$color1"
                onPress={() => onSubmit(value)}
              >
                {confirmLabel}
              </Button>
            </TamaguiAlertDialog.Action>
          </XStack>
        </TamaguiAlertDialog.Content>
      </TamaguiAlertDialog.Portal>
    </TamaguiAlertDialog>
  );
}
