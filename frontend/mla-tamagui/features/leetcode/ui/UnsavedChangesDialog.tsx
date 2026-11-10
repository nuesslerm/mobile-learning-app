import { AlertDialog } from '@lib/tamagui-design-system';

import { useUnsavedChangesRequestStore } from '../state/unsavedChangesRequestStore';

type Props = {
  onDismiss: () => void;
};

/**
 * Guards leaving the editor with edits in the buffer.
 *
 * The buffer is `useState` seeded from the problem's starter code and is not
 * persisted anywhere, so navigating away really does lose the work -- which is
 * why this exists at all.
 *
 * "Discard" takes a `navigate-away` snapshot before leaving, so the wording is
 * honest: the edits leave the editor but stay recoverable from Version history.
 */
export function UnsavedChangesDialog({ onDismiss }: Props) {
  const request = useUnsavedChangesRequestStore((s) => s.payload);
  const clearRequest = useUnsavedChangesRequestStore((s) => s.clear);

  const handleCancel = () => {
    request?.onCancel();
    clearRequest();
    onDismiss();
  };

  const handleDiscard = () => {
    request?.onDiscard();
    clearRequest();
    onDismiss();
  };

  return (
    <AlertDialog
      open
      onOpenChange={(next) => {
        if (!next) handleCancel();
      }}
      title="Leave without saving?"
      description="Your edits to this problem are not saved anywhere. Leaving discards them from the editor — a version is kept, so you can restore it from Version history."
      confirmLabel="Discard"
      cancelLabel="Keep editing"
      destructive
      onConfirm={handleDiscard}
      // The route is already the modal -- see ProblemDescriptionSheet.
      modal={false}
      testID="UnsavedChangesDialog"
    />
  );
}
