((spec) => {
  const rx = (p) => (p ? new RegExp(p[0], p[1]) : null);
  const idRe = rx(spec.id);
  const textRe = rx(spec.text);
  const css = spec.css || null;
  const SKIP = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE', 'HEAD', 'TITLE', 'META', 'LINK']);
  const field = (e) => e.tagName === 'INPUT' || e.tagName === 'TEXTAREA';
  const docOf = (f) => {
    try {
      return f.contentDocument && f.contentDocument.documentElement ? f.contentDocument : null;
    } catch (_) {
      return null;
    }
  };
  const shown = (e) => (e.innerText !== undefined ? e.innerText : e.textContent || '').trim();
  // a web view (an iframe, or a box holding one and no text) holds the field it hosts: the editor's lines, else its focused
  // input; null when it hosts none
  const hosted = (e) => {
    if (!(e.tagName === 'IFRAME' || e.firstElementChild) || shown(e)) return null;
    const f = e.tagName === 'IFRAME' ? e : e.querySelector('iframe');
    const doc = f && docOf(f);
    if (!doc) return null;
    const lines = doc.querySelectorAll('.cm-line');
    if (lines.length) return [...lines].map((l) => l.textContent).join('\n');
    const a = doc.activeElement;
    return a && field(a) ? a.value : null;
  };
  const textOf = (e) => {
    if (field(e)) return e.value;
    const h = hosted(e);
    if (h !== null) return h;
    return shown(e) || e.getAttribute('aria-label') || null;
  };
  // text matches what the element shows (an input's value) or its label; a web view's buffer is read as its text, never matched
  const test = (e) => {
    if (idRe) {
      const tid = e.getAttribute('data-testid');
      if (tid === null || !idRe.test(tid)) return false;
    }
    if (css && !e.matches(css)) return false;
    if (textRe) {
      const label = e.getAttribute('aria-label');
      if (!textRe.test(field(e) ? e.value : shown(e)) && !(label !== null && textRe.test(label))) return false;
    }
    return true;
  };
  const cut = (a, b) => ({ l: Math.max(a.l, b.l), t: Math.max(a.t, b.t), r: Math.min(a.r, b.r), b: Math.min(a.b, b.b) });
  // the on-screen part: the box clipped to its window, to every ancestor whose overflow clips, then to its frame's own part
  const onScreen = (e, frame) => {
    if (!frame.clip) return null;
    const win = e.ownerDocument.defaultView;
    if (!win || (e.checkVisibility && !e.checkVisibility({ visibilityProperty: true }))) return null;
    const box = e.getBoundingClientRect();
    let c = cut({ l: box.left, t: box.top, r: box.right, b: box.bottom }, { l: 0, t: 0, r: win.innerWidth, b: win.innerHeight });
    const doc = e.ownerDocument;
    let fixed = win.getComputedStyle(e).position === 'fixed';
    for (let a = e.parentElement; a && !fixed && a !== doc.body && a !== doc.documentElement; a = a.parentElement) {
      const s = win.getComputedStyle(a);
      if (s.overflowX !== 'visible' || s.overflowY !== 'visible') {
        const ar = a.getBoundingClientRect();
        const l = ar.left + a.clientLeft;
        const t = ar.top + a.clientTop;
        if (s.overflowX !== 'visible') c = cut(c, { l, t: -Infinity, r: l + a.clientWidth, b: Infinity });
        if (s.overflowY !== 'visible') c = cut(c, { l: -Infinity, t, r: Infinity, b: t + a.clientHeight });
      }
      if (s.position === 'fixed') fixed = true;
    }
    c = cut({ l: c.l + frame.x, t: c.t + frame.y, r: c.r + frame.x, b: c.b + frame.y }, frame.clip);
    return c.r > c.l && c.b > c.t ? c : null;
  };
  const visibility = spec.mode === 'one';
  // the deepest matches of a document, in document order: those with no match inside them (`all`, on screen or not, which a
  // wait observes), and the visible ones with no visible match inside them, with their on-screen part (`seen`)
  const deepest = (root, frame, all, seen) => {
    const visit = (el) => {
      let below = false;
      let belowSeen = false;
      for (const c of el.children) {
        const [b, s] = visit(c);
        below = below || b;
        belowSeen = belowSeen || s;
      }
      if ((below && belowSeen) || SKIP.has(el.tagName) || !test(el)) return [below, belowSeen];
      if (!below) all.push({ e: el, frame });
      if (visibility && !belowSeen) {
        const c = onScreen(el, frame);
        if (c) {
          seen.push({ e: el, frame, c });
          return [true, true];
        }
      }
      return [true, belowSeen];
    };
    if (root) visit(root);
  };
  const pathOf = (e) => {
    const parts = [];
    for (let n = e; n && n !== n.ownerDocument.documentElement; n = n.parentElement) {
      parts.unshift(`${n.localName}:nth-child(${[...n.parentElement.children].indexOf(n) + 1})`);
    }
    return [':root', ...parts].join(' > ');
  };
  const frames = [];
  const walk = (doc, frame, name) => {
    frames.push({ doc, frame: { ...frame, name } });
    for (const f of doc.querySelectorAll('iframe')) {
      const inner = docOf(f);
      if (!inner) continue;
      const clip = onScreen(f, frame);
      const r = f.getBoundingClientRect();
      walk(inner, { x: frame.x + r.left + f.clientLeft, y: frame.y + r.top + f.clientTop, clip }, 'editor');
    }
  };
  walk(document, { x: 0, y: 0, clip: { l: 0, t: 0, r: innerWidth, b: innerHeight } }, 'app');
  const rect = (c) => ({ left: Math.round(c.l), top: Math.round(c.t), right: Math.round(c.r), bottom: Math.round(c.b) });
  const box = (e, frame) => {
    const b = e.getBoundingClientRect();
    return { l: b.left + frame.x, t: b.top + frame.y, r: b.right + frame.x, b: b.bottom + frame.y };
  };
  if (spec.mode === 'tree') {
    const out = [];
    for (const { doc, frame } of frames) {
      for (const e of doc.body ? doc.body.querySelectorAll('*') : []) {
        if (out.length >= 500 || SKIP.has(e.tagName)) continue;
        const tid = e.getAttribute('data-testid');
        const label = e.getAttribute('aria-label');
        const text = [...e.childNodes].filter((n) => n.nodeType === 3).map((n) => n.textContent).join('').trim();
        if (tid === null && label === null && !text && !field(e)) continue;
        const c = onScreen(e, frame);
        if (!c) continue;
        out.push({ id: tid, label, text: (field(e) ? e.value : text).slice(0, 80), bounds: rect(c), frame: frame.name });
      }
    }
    return { tree: out };
  }
  const all = [];
  const seen = [];
  for (const { doc, frame } of frames) deepest(doc.documentElement, frame, all, seen);
  const els = all.map((m) => m.e);
  const pick = seen[0];
  if (!pick) return { hit: null, count: els.length, els };
  const hit = { bounds: rect(pick.c), full: rect(box(pick.e, pick.frame)), text: textOf(pick.e), frame: pick.frame.name, path: pathOf(pick.e) };
  return { hit, count: els.length, els };
})(WF_FIND_SPEC);
