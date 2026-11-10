(() => {
  const refs = {};
  let counter = 0;
  const norm = (s) => s.replace(/\s+/g, ' ').trim();
  const SKIP = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE', 'HEAD', 'META', 'LINK', 'TITLE']);
  const hidden = (el) => {
    if (SKIP.has(el.tagName) || el.hidden || el.getAttribute('aria-hidden') === 'true') return true;
    const cs = getComputedStyle(el);
    return cs.display === 'none' || cs.visibility === 'hidden';
  };
  const labelFor = (el) => {
    if (el.id) {
      const l = document.querySelector('label[for="' + CSS.escape(el.id) + '"]');
      if (l) return norm(l.textContent);
    }
    const wrap = el.closest('label');
    if (wrap) return norm(Array.from(wrap.childNodes).filter((n) => n !== el && !(n.nodeType === 1 && n.contains(el))).map((n) => n.textContent).join(' '));
    return '';
  };
  const ariaName = (el) => {
    const label = el.getAttribute('aria-label');
    if (label) return norm(label);
    const by = el.getAttribute('aria-labelledby');
    if (by) return norm(by.split(/\s+/).map((id) => (document.getElementById(id) || {}).textContent || '').join(' '));
    return '';
  };
  const INPUT_ROLES = { checkbox: 'checkbox', radio: 'radio', button: 'button', submit: 'button', reset: 'button', image: 'button', range: 'slider', search: 'searchbox' };
  const TAG_ROLES = { A: 'link', BUTTON: 'button', H1: 'heading', H2: 'heading', H3: 'heading', H4: 'heading', H5: 'heading', H6: 'heading', UL: 'list', OL: 'list', LI: 'listitem', P: 'paragraph', NAV: 'navigation', MAIN: 'main', HEADER: 'banner', FOOTER: 'contentinfo', ASIDE: 'complementary', TABLE: 'table', TBODY: 'rowgroup', THEAD: 'rowgroup', TFOOT: 'rowgroup', TR: 'row', TD: 'cell', TH: 'columnheader', SELECT: 'combobox', OPTION: 'option', TEXTAREA: 'textbox', IMG: 'img', DIALOG: 'dialog', FORM: 'form', SECTION: null, ARTICLE: 'article', HR: 'separator', PROGRESS: 'progressbar' };
  const roleOf = (el) => {
    const explicit = el.getAttribute('role');
    if (explicit) return explicit.split(/\s+/)[0];
    if (el.tagName === 'INPUT') return INPUT_ROLES[el.type] || 'textbox';
    if (el.tagName === 'A' && !el.hasAttribute('href')) return null;
    if (el.tagName === 'IMG' && el.getAttribute('alt') === '') return 'presentation';
    return TAG_ROLES[el.tagName] || null;
  };
  const FROM_CONTENT = new Set(['button', 'link', 'heading', 'cell', 'columnheader', 'rowheader', 'option', 'tab', 'menuitem', 'checkbox', 'radio']);
  const text = (t) => ({ kind: 'text', text: t });
  // Merge adjacent text and collapse whitespace.
  const tidy = (items) => {
    const out = [];
    for (const it of items) {
      const last = out[out.length - 1];
      if (it.kind === 'text' && last && last.kind === 'text') last.text += it.text;
      else out.push({ ...it });
    }
    return out.map((it) => (it.kind === 'text' ? { ...it, text: norm(it.text) } : it)).filter((it) => it.kind !== 'text' || it.text);
  };
  const walk = (el) => {
    if (hidden(el)) return [];
    const role = roleOf(el);
    if (role === 'presentation' || role === 'none') return [];
    const kids = [];
    if (el.tagName !== 'SELECT' || true) {
      for (const n of el.childNodes) {
        if (n.nodeType === 3) kids.push(text(n.textContent));
        else if (n.nodeType === 1) kids.push(...walk(n));
      }
    }
    let items = tidy(kids);
    if (role === null) {
      // An empty wrapper prints nothing, but Playwright still numbers it: a ghost keeps the refs of what follows aligned.
      if (items.length === 0) return [{ kind: 'ghost' }];
      const real = items.filter((i) => i.kind !== 'ghost');
      if (real.length <= 1) return items;
      if (real.length === items.length && real.every((i) => i.kind === 'text')) return [text(real.map((i) => i.text).join(' '))];
      return [{ kind: 'node', role: 'generic', el, name: '', props: [], attrs: {}, children: items }];
    }
    let name = ariaName(el);
    const attrs = {};
    if (el.tagName === 'IMG') name = name || el.getAttribute('alt') || '';
    if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT') name = name || labelFor(el) || el.getAttribute('placeholder') || '';
    if (FROM_CONTENT.has(role) && !name) {
      if (el.tagName === 'INPUT') name = el.value || '';
      else {
        name = items.filter((i) => i.kind === 'text').map((i) => i.text).join(' ');
        items = items.filter((i) => i.kind !== 'text');
      }
    }
    if (role === 'heading') attrs.level = Number(el.tagName[1]) || Number(el.getAttribute('aria-level')) || 2;
    if (el.checked || el.getAttribute('aria-checked') === 'true') attrs.checked = true;
    if (el.disabled || el.getAttribute('aria-disabled') === 'true') attrs.disabled = true;
    if (el.tagName === 'OPTION' && el.selected) attrs.selected = true;
    const props = [];
    if (role === 'link' && el.getAttribute('href') !== null) props.push(['/url', el.getAttribute('href')]);
    if (el.tagName === 'INPUT' && !(el.type in INPUT_ROLES) && el.value) items = [text(el.value)];
    if (el.tagName === 'TEXTAREA') items = el.value ? [text(el.value)] : [];
    if (el.tagName === 'SELECT') items = Array.from(el.options).map((o) => walk(o)[0]).filter(Boolean);
    return [{ kind: 'node', role, el, name, props, attrs, children: items }];
  };
  const yaml = (t) => (/^\s|\s$|[\n\r\t]|^[-?:,\[\]{}#&*!|>'"%@`]|: | #/.test(t) ? JSON.stringify(t) : t);
  const lines = [];
  const emit = (n, depth) => {
    const pad = '  '.repeat(depth) + '- ';
    if (n.kind === 'ghost') {
      counter += 1;
      return;
    }
    if (n.kind === 'text') {
      lines.push(pad + 'text: ' + yaml(n.text));
      return;
    }
    const isOption = n.role === 'option';
    let head = n.role + (n.name ? ' "' + n.name.replace(/"/g, '\\"') + '"' : '');
    if (document.activeElement === n.el) head += ' [active]';
    for (const k of ['checked', 'disabled']) if (n.attrs[k]) head += ' [' + k + ']';
    if (n.attrs.selected) head += ' [selected]';
    if (n.attrs.level) head += ' [level=' + n.attrs.level + ']';
    if (!isOption) {
      const ref = 'e' + ++counter;
      refs[ref] = n.el;
      head += ' [ref=' + ref + ']';
      if (getComputedStyle(n.el).cursor === 'pointer') head += ' [cursor=pointer]';
    }
    const kids = n.children;
    const shown = kids.filter((c) => c.kind !== 'ghost');
    const single = shown.length === 1 && kids.length === 1 && kids[0].kind === 'text' && n.props.length === 0;
    if (single) {
      lines.push(pad + head + ': ' + yaml(kids[0].text));
    } else if (shown.length || n.props.length) {
      lines.push(pad + head + ':');
      for (const [k, v] of n.props) lines.push('  '.repeat(depth + 1) + '- ' + k + ': ' + v);
      for (const c of kids) emit(c, depth + 1);
    } else lines.push(pad + head);
  };
  const root = document.body ? walk(document.body) : [];
  const top = root.length === 1 ? root : [{ kind: 'node', role: 'generic', el: document.body, name: '', props: [], attrs: {}, children: root }];
  for (const n of top) emit(n, 0);
  window.__wfRefs = refs;
  return lines.join('\n');
})()
