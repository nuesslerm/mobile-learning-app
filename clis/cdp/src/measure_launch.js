function measureLaunchPage() {
  const MIN_INTERACTIVE_PX = 4;
  const clippedAwayByAncestor = (el) => {
    const r = el.getBoundingClientRect();
    const unscrollable = (value) => value === "hidden" || value === "clip";
    for (let p = el.parentElement; p; p = p.parentElement) {
      const cs = getComputedStyle(p);
      if (cs.display === "contents") continue;
      const pr = p.getBoundingClientRect();
      const overlapX = Math.min(r.right, pr.right) - Math.max(r.left, pr.left);
      const overlapY = Math.min(r.bottom, pr.bottom) - Math.max(r.top, pr.top);
      if (unscrollable(cs.overflowX) && overlapX <= 0) return true;
      if (unscrollable(cs.overflowY) && overlapY <= 0) return true;
    }
    return false;
  };
  const explainNotPainting = (el) => {
    for (let p = el; p; p = p.parentElement) {
      const cs = getComputedStyle(p);
      const where = p === el ? "self" : (p.tagName || "?").toLowerCase();
      const cls = String(p.className || "").slice(0, 24);
      const at = cls ? `${where}.${cls}` : where;
      if (cs.visibility === "hidden") return `visibility:hidden@${at}`;
      if (cs.display === "none") return `display:none@${at}`;
      if (Number(cs.opacity || "1") === 0) return `opacity:0@${at}`;
      const cv = cs.contentVisibility;
      if (cv === "hidden") return `content-visibility:hidden@${at}`;
      if (cs.display === "contents") continue;
      const pr = p.getBoundingClientRect();
      if (p !== el && (pr.width <= 0 || pr.height <= 0)) {
        return `zero-size-ancestor@${at}`;
      }
    }
    const rects = el.getClientRects;
    if (typeof rects === "function" && rects.call(el).length === 0) {
      return "no-client-rects";
    }
    if (typeof document.elementFromPoint === "function") {
      const r = el.getBoundingClientRect();
      const cx = Math.min(
        Math.max(r.left + r.width / 2, 0),
        window.innerWidth - 1
      );
      const cy = Math.min(
        Math.max(r.top + r.height / 2, 0),
        window.innerHeight - 1
      );
      const hit = document.elementFromPoint(cx, cy);
      if (hit && hit !== el && !el.contains(hit) && !hit.contains(el)) {
        const cls = String(hit.className || "").slice(0, 24);
        const tag = (hit.tagName || "?").toLowerCase();
        return `occluded-by=${cls ? `${tag}.${cls}` : tag}`;
      }
    }
    return "unexplained";
  };
  const centreInViewport = (el) => {
    const r = el.getBoundingClientRect();
    const x = r.left + r.width / 2;
    const y = r.top + r.height / 2;
    if (x < 0 || y < 0) return null;
    if (x >= window.innerWidth || y >= window.innerHeight) return null;
    return { x, y };
  };
  const paintedAtCentre = (el) => {
    const centre = centreInViewport(el);
    if (!centre) return false;
    const hit = document.elementFromPoint(centre.x, centre.y);
    if (!hit) return false;
    return hit === el || el.contains(hit) || hit.contains(el);
  };
  const occludedByPageContent = (el) => {
    if (typeof document.elementFromPoint !== "function") return false;
    const centre = centreInViewport(el);
    if (!centre) return false;
    const hit = document.elementFromPoint(centre.x, centre.y);
    if (!hit) return false;
    return hit !== el && !el.contains(hit) && !hit.contains(el);
  };
  const isVisible = (el) => {
    const cs = getComputedStyle(el);
    if (cs.visibility === "hidden" || cs.display === "none" || Number(cs.opacity || "1") === 0) {
      return false;
    }
    const withCheck = el;
    if (typeof withCheck.checkVisibility === "function") {
      const rendered = withCheck.checkVisibility({
        checkOpacity: true,
        opacityProperty: true,
        checkVisibilityCSS: true,
        visibilityProperty: true,
        contentVisibilityAuto: true
      });
      if (!rendered) return false;
    }
    const r = el.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) return false;
    return !clippedAwayByAncestor(el);
  };
  const readFabRect = (host) => {
    const names = [
      "data-fab-top",
      "data-fab-left",
      "data-fab-width",
      "data-fab-height"
    ];
    if (!names.every((n) => host.hasAttribute(n))) return null;
    const top = Number(host.getAttribute("data-fab-top"));
    const left = Number(host.getAttribute("data-fab-left"));
    const width = Number(host.getAttribute("data-fab-width"));
    const height = Number(host.getAttribute("data-fab-height"));
    if (![top, left, width, height].every((n) => Number.isFinite(n)) || width <= 0 || height <= 0) {
      return null;
    }
    return { top, left, width, height };
  };
  const readConsentBanner = () => {
    const bannerRe = /cookie|consent|privacy|tracking|gdpr/i;
    const clickableRe = /^(accept|allow|agree|reject|decline|refuse|deny|only necessary|necessary only|essential|manage|continue without accepting)/i;
    const clickableSelector = 'button, [role="button"], a, input[type="button"], input[type="submit"]';
    for (const el of Array.from(document.querySelectorAll("*"))) {
      if (!isVisible(el)) continue;
      const ownText = (el.textContent ?? "").trim();
      if (!bannerRe.test(ownText)) continue;
      const labels = [];
      for (const clickable of Array.from(
        el.querySelectorAll(clickableSelector)
      )) {
        if (!isVisible(clickable)) continue;
        const label = (clickable.textContent ?? "").trim();
        if (!clickableRe.test(label)) continue;
        if (!labels.includes(label)) labels.push(label);
        if (labels.length === 3) break;
      }
      if (labels.length > 0) {
        return {
          bannerFound: true,
          bannerText: ownText.slice(0, 80),
          clickableLabels: labels.join(", ")
        };
      }
    }
    return { bannerFound: false };
  };
  const candidates = Array.from(
    document.querySelectorAll("input, button, a, textarea, select")
  ).filter(isVisible);
  const measure = (ratios) => {
    const host = document.getElementById("vnc-open-url-host");
    const elements = candidates.map((el) => {
      const r = el.getBoundingClientRect();
      const visibleWidth = Math.max(
        0,
        Math.min(r.right, window.innerWidth) - Math.max(r.left, 0)
      );
      return {
        tag: el.tagName.toLowerCase(),
        cls: String(el.className || "").slice(0, 60),
        ratio: ratios.get(el) ?? 0,
        hRatio: r.width > 0 ? visibleWidth / r.width : 1,
        inFold: r.top >= 0 && r.bottom <= window.innerHeight,
        reason: (ratios.get(el) ?? 0) > 0 ? "" : explainNotPainting(el),
        painted: (ratios.get(el) ?? 0) === 0 && typeof document.elementFromPoint === "function" && paintedAtCentre(el),
        occluded: (ratios.get(el) ?? 0) === 0 && occludedByPageContent(el),
        tiny: r.width < MIN_INTERACTIVE_PX || r.height < MIN_INTERACTIVE_PX,
        left: Math.round(r.left),
        right: Math.round(r.right),
        top: Math.round(r.top),
        bottom: Math.round(r.bottom),
        width: Math.round(r.width)
      };
    });
    return {
      checked: elements.length,
      devicePixelRatio: window.devicePixelRatio,
      screenWidth: window.screen.width,
      bodyWidth: document.body.getBoundingClientRect().width,
      // A page's own vertical scrollbar consumes the layout viewport, so the
      // raw `clientWidth` (layoutViewport.clientWidth) under-reports the real
      // device width on any page tall enough to scroll. Recover it: innerWidth
      // spans the full viewport including the scrollbar, while
      // documentElement.clientWidth is the content box it steals from. Zero on
      // a non-scrolling page, so the effective width is just the layout width.
      scrollbarWidth: window.innerWidth - document.documentElement.clientWidth,
      elements,
      hostPresent: host !== null,
      fab: host ? readFabRect(host) : null,
      ...readConsentBanner()
    };
  };
  if (candidates.length === 0) {
    return Promise.resolve(measure(/* @__PURE__ */ new Map()));
  }
  return new Promise((resolve) => {
    const ratios = /* @__PURE__ */ new Map();
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          ratios.set(entry.target, entry.intersectionRatio);
        }
        if (ratios.size < candidates.length) return;
        observer.disconnect();
        resolve(measure(ratios));
      },
      {
        root: null,
        threshold: [0, 0.25, 0.5, 0.75, 0.9, 0.95, 0.98, 1]
      }
    );
    for (const el of candidates) observer.observe(el);
  });
}

