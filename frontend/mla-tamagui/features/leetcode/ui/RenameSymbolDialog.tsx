import { PromptDialog } from '@lib/tamagui-design-system';

import { useRenameSymbolRequestStore } from '../state/renameSymbolRequestStore';

type Props = {
  /** Route-param fallback so a cold deep link (no live request in the store)
   *  still shows a populated dialog -- see docs/overlays-as-routes.md. */
  initialTarget?: string;
  onDismiss: () => void;
};

export function RenameSymbolDialog({ initialTarget, onDismiss }: Props) {
  const request = useRenameSymbolRequestStore((s) => s.payload);
  const clearRequest = useRenameSymbolRequestStore((s) => s.clear);
  const target = request?.target ?? initialTarget ?? '';

  const handleCancel = () => {
    request?.onCancel();
    clearRequest();
    onDismiss();
  };

  const handleConfirm = (newName: string) => {
    if (newName) request?.onConfirm(newName);
    else request?.onCancel();
    clearRequest();
    onDismiss();
  };

  return (
    <PromptDialog
      open
      onOpenChange={(next) => {
        if (!next) handleCancel();
      }}
      title="Rename symbol"
      description={target ? `Rename all occurrences of "${target}"` : undefined}
      initialValue={target}
      placeholder="New name"
      confirmLabel="Rename"
      onSubmit={handleConfirm}
      // The route is already the modal -- see ProblemDescriptionSheet.
      modal={false}
      testID="RenameSymbolDialog"
    />
  );
}
