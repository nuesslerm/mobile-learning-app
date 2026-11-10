export type AlertButton = {
  text?: string;
  style?: 'default' | 'cancel' | 'destructive';
  onPress?: () => void;
};
export type AlertOptions = { cancelable?: boolean; onDismiss?: () => void };
export type PendingAlert = {
  title: string;
  message?: string;
  buttons: AlertButton[];
  options?: AlertOptions;
};

const BLUE = '#007aff';
const RED = '#ff3b30';

function styled(el: HTMLElement, css: Partial<CSSStyleDeclaration>) {
  Object.assign(el.style, css);
  return el;
}

/** A native-looking alert: dim backdrop, 270 px card, system-blue buttons, the cancel one bold. */
export function buildAlertDialog(
  doc: Document,
  { title, message, buttons, options }: PendingAlert,
  answer: (index: number | null) => void
): HTMLElement {
  const dark = doc.defaultView?.matchMedia?.('(prefers-color-scheme: dark)').matches === true;
  const ink = dark ? '#ffffff' : '#000000';
  const line = dark ? '#545458' : '#c6c6c8';

  const backdrop = styled(doc.createElement('div'), {
    position: 'fixed',
    inset: '0',
    zIndex: '2147483000',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    background: 'rgba(0,0,0,0.4)',
    fontFamily: '-apple-system, system-ui, sans-serif',
  });
  backdrop.setAttribute('data-testid', 'LabAlert');
  backdrop.addEventListener('click', (event) => {
    if (event.target === backdrop && options?.cancelable) answer(null);
  });

  const card = styled(doc.createElement('div'), {
    width: '270px',
    borderRadius: '14px',
    overflow: 'hidden',
    background: dark ? 'rgba(44,44,46,0.96)' : 'rgba(242,242,247,0.96)',
    color: ink,
    textAlign: 'center',
  });
  card.setAttribute('role', 'alertdialog');
  card.setAttribute('aria-label', title);

  const text = styled(doc.createElement('div'), { padding: '18px 16px 16px' });
  const heading = styled(doc.createElement('div'), { fontSize: '17px', fontWeight: '600' });
  heading.setAttribute('data-testid', 'LabAlert-Title');
  heading.textContent = title;
  text.appendChild(heading);
  if (message) {
    const body = styled(doc.createElement('div'), { fontSize: '13px', marginTop: '4px' });
    body.setAttribute('data-testid', 'LabAlert-Message');
    body.textContent = message;
    text.appendChild(body);
  }
  card.appendChild(text);

  const sideBySide = buttons.length === 2;
  const row = styled(doc.createElement('div'), {
    display: 'flex',
    flexDirection: sideBySide ? 'row' : 'column',
    borderTop: `0.5px solid ${line}`,
  });
  buttons.forEach((button, index) => {
    const el = styled(doc.createElement('div'), {
      flex: '1',
      minHeight: '44px',
      boxSizing: 'border-box',
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      fontSize: '17px',
      fontWeight: button.style === 'cancel' ? '600' : '400',
      color: button.style === 'destructive' ? RED : BLUE,
      cursor: 'pointer',
      borderTop: !sideBySide && index > 0 ? `0.5px solid ${line}` : '',
      borderLeft: sideBySide && index > 0 ? `0.5px solid ${line}` : '',
    });
    el.setAttribute('role', 'button');
    el.setAttribute('data-testid', `LabAlert-Button-${index}`);
    el.textContent = button.text ?? 'OK';
    el.addEventListener('click', () => answer(index));
    row.appendChild(el);
  });
  card.appendChild(row);
  backdrop.appendChild(card);
  return backdrop;
}
