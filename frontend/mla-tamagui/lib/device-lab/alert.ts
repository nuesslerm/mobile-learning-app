import { Alert } from 'react-native';

import { buildAlertDialog, type AlertButton, type AlertOptions, type PendingAlert } from './alertDialog';
import type { DeviceLab } from './index';

type Pending = PendingAlert & { record: DeviceLab['alerts'][number] };

/**
 * Replaces `Alert.alert`, which react-native-web leaves empty, so a confirm flow no longer silently
 * skips. Each alert is recorded in `lab.alerts` with the button that answered it, shown one at a
 * time like the system queue, and answered through the dialog's own buttons.
 */
export function installLabAlert(lab: DeviceLab, doc: Document | undefined = globalThis.document): void {
  const queue: Pending[] = [];
  let showing: HTMLElement | null = null;

  const next = () => {
    const pending = queue.shift();
    if (!pending || !doc?.body) return;
    const close = (index: number | null) => {
      showing?.remove();
      showing = null;
      pending.record.answered = index ?? undefined;
      if (index === null) pending.options?.onDismiss?.();
      else pending.buttons[index]?.onPress?.();
      next();
    };
    showing = buildAlertDialog(doc, pending, close);
    doc.body.appendChild(showing);
  };

  Alert.alert = (title: string, message?: string, buttons?: AlertButton[], options?: AlertOptions) => {
    const shown = buttons?.length ? buttons : [{ text: 'OK' }];
    const record = { title, message, buttons: shown.map((button) => button.text ?? 'OK') };
    lab.alerts.push(record);
    queue.push({ title, message, buttons: shown, options, record });
    if (!showing) next();
  };
}
