// jsdom leaves two browser behaviours out that an embedded page (the CodeMirror editor) depends on:
// it never loads `iframe[srcdoc]`, and cross-frame messages arrive with `source: null`.
const loaded = new WeakMap<HTMLIFrameElement, string>();

function loadSrcdoc(frame: HTMLIFrameElement): void {
  const html = frame.getAttribute('srcdoc');
  const doc = frame.contentDocument;
  const win = frame.contentWindow;
  if (html == null || !doc || !win || loaded.get(frame) === html) return;
  loaded.set(frame, html);

  const top = window;
  Object.defineProperty(win, 'parent', {
    configurable: true,
    value: {
      postMessage: (data: unknown, origin?: string) =>
        setTimeout(() => {
          top.dispatchEvent(
            new MessageEvent('message', { data, origin: origin === '*' ? 'null' : origin, source: win })
          );
        }, 0),
    },
  });

  // CodeMirror measures the caret through Range rects, which jsdom does not implement.
  const rangeProto = (win as unknown as { Range?: { prototype: Range } }).Range?.prototype;
  if (rangeProto && !rangeProto.getClientRects) {
    rangeProto.getClientRects = () => [] as unknown as DOMRectList;
    rangeProto.getBoundingClientRect = () =>
      ({ x: 0, y: 0, width: 0, height: 0, top: 0, left: 0, right: 0, bottom: 0, toJSON: () => ({}) }) as DOMRect;
  }

  doc.open();
  doc.write(html);
  doc.close();
}

function scan(root: ParentNode): void {
  root.querySelectorAll('iframe[srcdoc]').forEach((frame) => loadSrcdoc(frame as HTMLIFrameElement));
}

export function installFrameFidelity(): void {
  new MutationObserver((records) => {
    for (const record of records) {
      if (record.type === 'attributes' && record.target instanceof HTMLIFrameElement) {
        loadSrcdoc(record.target);
      }
      for (const node of Array.from(record.addedNodes)) {
        if (node instanceof HTMLIFrameElement) loadSrcdoc(node);
        if (node instanceof Element) scan(node);
      }
    }
  }).observe(document, { subtree: true, childList: true, attributes: true, attributeFilter: ['srcdoc'] });
}
