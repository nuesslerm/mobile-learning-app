import type { ReactNode } from 'react';

export interface PairingScreenProps {
  permission: 'loading' | 'prompt' | 'denied' | 'granted';
  status: 'scanning' | 'connecting' | 'error';
  error?: string;
  topInset: number;
  bottomInset: number;
  keyboardInset: number;
  pasteVisible: boolean;
  camera?: ReactNode;
  onBack(): void;
  onCamera(): void;
  onPaste(): void;
  onCancel(): void;
  onSubmit(value: string): void;
  onRetry(): void;
}

export interface PairingCodeSheetProps {
  visible: boolean;
  bottomInset: number;
  keyboardInset: number;
  onCancel(): void;
  onSubmit(value: string): void;
}
