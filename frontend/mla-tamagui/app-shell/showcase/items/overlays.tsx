import {
  AlertDialog,
  Button,
  PromptDialog,
  Text,
} from '@lib/tamagui-design-system';
import { Trash2 } from '@lib/tamagui-design-system/icons';
import { useState } from 'react';

import { Compare } from '../Compare';

export function AlertDialogItem() {
  const [open, setOpen] = useState(false);
  return (
    <Compare
      testID="ShowcaseAlertDialog"
      tamagui={
        <>
          <Button
            variant="destructive"
            icon={Trash2}
            testID="ShowcaseDestructive"
            onPress={() => setOpen(true)}
          >
            Open AlertDialog
          </Button>
          <AlertDialog
            open={open}
            onOpenChange={setOpen}
            title="Delete everything?"
            description="This confirms the dialog renders through the DS wrapper."
            confirmLabel="Delete"
            destructive
            onConfirm={() => setOpen(false)}
            testID="ShowcaseAlert"
          />
        </>
      }
    />
  );
}

export function PromptDialogItem() {
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState('');
  return (
    <Compare
      testID="ShowcasePromptDialog"
      tamagui={
        <>
          <Button
            variant="outline"
            testID="ShowcasePromptOpen"
            onPress={() => setOpen(true)}
          >
            Open PromptDialog
          </Button>
          <Text variant="muted" testID="ShowcasePromptValue">
            {value === '' ? 'Nothing entered yet.' : `Entered: ${value}`}
          </Text>
          <PromptDialog
            open={open}
            onOpenChange={setOpen}
            title="Rename"
            description="The prompt returns the typed value."
            placeholder="New name"
            confirmLabel="Save"
            onSubmit={(next) => {
              setValue(next);
              setOpen(false);
            }}
            testID="ShowcasePrompt"
          />
        </>
      }
    />
  );
}
