import type { ReactElement } from 'react';

export type MenuPanelPopoverProps = {
  trigger: ReactElement;
  content: ReactElement;
  width: number;
  open: boolean;
  onClose: () => void;
};

export function supportsMenuPanelPopover(): boolean {
  return false;
}

export function MenuPanelPopover({ trigger }: MenuPanelPopoverProps) {
  return trigger;
}
