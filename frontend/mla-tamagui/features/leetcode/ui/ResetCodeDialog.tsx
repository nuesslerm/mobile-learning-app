import { AlertDialog } from '@lib/tamagui-design-system';

import { useResetCodeRequestStore } from '../state/resetCodeRequestStore';

type Props = {
  onDismiss: () => void;
};

export function ResetCodeDialog({ onDismiss }: Props) {
  const request = useResetCodeRequestStore((s) => s.payload);
  const clearRequest = useResetCodeRequestStore((s) => s.clear);

  const handleCancel = () => {
    request?.onCancel();
    clearRequest();
    onDismiss();
  };

  const handleConfirm = () => {
    request?.onConfirm();
    clearRequest();
    onDismiss();
  };

  return (
    <AlertDialog
      open
      onOpenChange={(next) => {
        if (!next) handleCancel();
      }}
      title="Reset to starter code?"
      description="This discards your current edits for this problem and restores the starter template. This cannot be undone."
      confirmLabel="Reset"
      destructive
      onConfirm={handleConfirm}
      // The route is already the modal -- see ProblemDescriptionSheet.
      modal={false}
      testID="ResetCodeDialog"
    />
  );
}
