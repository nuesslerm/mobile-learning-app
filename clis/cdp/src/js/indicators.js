(() => {
// On-page markers for what the harness did, drawn so a screenshot or a recording reads like a storyboard: every tap,
// drag, swipe and stroke carries its action number, the newest one is the brightest, a tap ripples when it lands, a
// drag shows its direction with an arrowhead and a finger that travels the path, a stroke draws itself on. The root
// element of each kind keeps its data-testid and (for taps) `style.background` = the colour, which the tests read.
const Z = 2147483646;
const FONT = "600 11px/1 system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif";
const SHADOW = "0 1px 3px rgba(0,0,0,.45), 0 0 0 1px rgba(255,255,255,.55)";
const keep = (w, key, el, max) => {
  const arr = (w[key] = w[key] ?? []);
  arr.push(el);
  for (const older of arr.slice(0, -1)) older.style.opacity = "0.45";
  while (arr.length > max) arr.shift()?.remove();
  return () => {
    el.remove();
    const i = arr.indexOf(el);
    if (i !== -1) arr.splice(i, 1);
  };
};
const fadeLater = (el, lingerMs, fadeMs, removeNow) => {
  setTimeout(() => {
    el.style.transition = `opacity ${fadeMs}ms ease-in`;
    el.style.opacity = "0";
    setTimeout(removeNow, fadeMs);
  }, lingerMs);
};
const svgEl = (name, attrs) => {
  const el = document.createElementNS("http://www.w3.org/2000/svg", name);
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, String(v));
  return el;
};
const overlay = (testid, fadeMs) => {
  const svg = svgEl("svg", { "data-testid": testid, width: "100%", height: "100%" });
  svg.style.cssText = `position:fixed;left:0;top:0;width:100vw;height:100vh;pointer-events:none;z-index:${Z};opacity:0.95;overflow:visible;transition:opacity ${fadeMs}ms ease-in`;
  return svg;
};
// A numbered badge beside a point: the action id, so a frame tells which step it shows.
const badge = (svg, x, y, n, color) => {
  if (!n) return;
  const g = svgEl("g", { transform: `translate(${x + 13} ${y - 13})` });
  g.appendChild(svgEl("circle", { r: 9, fill: "#fff", stroke: color, "stroke-width": 2 }));
  const t = svgEl("text", { "text-anchor": "middle", "dominant-baseline": "central", fill: color, "font-size": 11, "font-weight": 700, "font-family": "system-ui, sans-serif" });
  t.textContent = String(n);
  g.appendChild(t);
  svg.appendChild(g);
};
const arrowHead = (svg, color, id) => {
  const defs = svgEl("defs", {});
  const m = svgEl("marker", { id, viewBox: "0 0 10 10", refX: 7, refY: 5, markerWidth: 6, markerHeight: 6, orient: "auto-start-reverse" });
  m.appendChild(svgEl("path", { d: "M 0 0 L 10 5 L 0 10 z", fill: color }));
  defs.appendChild(m);
  svg.appendChild(defs);
};

const tap = ({ x, y, n, color, durationMs, max, lingerMs, fadeMs }) => {
  const w = window;
  const dot = document.createElement("div");
  dot.setAttribute("data-testid", "wf-browser-tap-indicator");
  dot.style.cssText = `position:fixed;left:${x - 13}px;top:${y - 13}px;width:26px;height:26px;border-radius:50%;background:${color};opacity:0.92;border:2px solid #fff;box-shadow:${SHADOW};pointer-events:none;z-index:${Z + 1};display:flex;align-items:center;justify-content:center;color:#fff;font:${FONT};box-sizing:border-box`;
  if (n) dot.textContent = String(n);
  const ripple = document.createElement("div");
  ripple.style.cssText = `position:fixed;left:${x - 13}px;top:${y - 13}px;width:26px;height:26px;border-radius:50%;border:2px solid ${color};pointer-events:none;z-index:${Z};opacity:0.8;box-sizing:border-box`;
  document.body.appendChild(ripple);
  document.body.appendChild(dot);
  ripple.animate([{ transform: "scale(1)", opacity: 0.8 }, { transform: "scale(3)", opacity: 0 }], { duration: durationMs, easing: "ease-out" }).onfinish = () => ripple.remove();
  dot.animate([{ transform: "scale(0.6)" }, { transform: "scale(1.15)" }, { transform: "scale(1)" }], { duration: Math.min(durationMs, 300), easing: "ease-out" });
  fadeLater(dot, lingerMs, fadeMs, keep(w, "__mcpTapMarkers", dot, max));
};

const drag = ({ startX, startY, endX, endY, n, color, durationMs, max, lingerMs, fadeMs }) => {
  const w = window;
  const svg = overlay("wf-browser-drag-indicator", fadeMs);
  const uid = Math.random().toString(36).slice(2);
  const dx = endX - startX;
  const dy = endY - startY;
  const length = Math.hypot(dx, dy) || 1;
  const angle = Math.atan2(dy, dx) * 180 / Math.PI;
  // a comet: a tail that starts thin and faint where the finger landed and ends thick and solid under the arrowhead
  const defs = svgEl("defs", {});
  const grad = svgEl("linearGradient", { id: `wf-grad-${uid}`, gradientUnits: "userSpaceOnUse", x1: startX, y1: startY, x2: endX, y2: endY });
  grad.appendChild(svgEl("stop", { offset: "0", "stop-color": color, "stop-opacity": 0.35 }));
  grad.appendChild(svgEl("stop", { offset: "1", "stop-color": color, "stop-opacity": 1 }));
  defs.appendChild(grad);
  svg.appendChild(defs);
  const head = 22;
  const body = Math.max(length - head, 4);
  const tail = svgEl("g", { transform: `translate(${startX} ${startY}) rotate(${angle})` });
  tail.appendChild(svgEl("path", { d: `M 0 -1.5 L ${body} -6 L ${body} 6 L 0 1.5 Z`, fill: "#fff", "fill-opacity": 0.85, stroke: "#fff", "stroke-width": 3, "stroke-linejoin": "round" }));
  tail.appendChild(svgEl("path", { d: `M 0 -1.5 L ${body} -6 L ${body} 6 L 0 1.5 Z`, fill: `url(#wf-grad-${uid})` }));
  tail.appendChild(svgEl("path", { d: `M ${body - 2} -15 L ${body + head} 0 L ${body - 2} 15 Z`, fill: color, stroke: "#fff", "stroke-width": 2.5, "stroke-linejoin": "round" }));
  svg.appendChild(tail);
  svg.appendChild(svgEl("circle", { cx: startX, cy: startY, r: 8, fill: "#fff", "fill-opacity": 0.8, stroke: color, "stroke-width": 2.5 }));
  const finger = svgEl("circle", { "data-testid": "wf-browser-drag-indicator-end", cx: startX, cy: startY, r: 11, fill: color, stroke: "#fff", "stroke-width": 2 });
  finger.style.filter = "drop-shadow(0 1px 2px rgba(0,0,0,.4))";
  svg.appendChild(finger);
  badge(svg, startX, startY, n, color);
  document.body.appendChild(svg);
  // the finger travels the path and lifts at the end, so the arrowhead stays readable in a still frame
  finger.animate([{ transform: "translate(0,0)", opacity: 1 }, { transform: `translate(${dx}px,${dy}px)`, opacity: 1, offset: 0.85 }, { transform: `translate(${dx}px,${dy}px)`, opacity: 0 }], { duration: durationMs, easing: "ease-in-out", fill: "forwards" });
  fadeLater(svg, lingerMs, fadeMs, keep(w, "__mcpDragMarkers", svg, max));
};

const stroke = ({ points, n, color, durationMs, max, lingerMs, fadeMs }) => {
  const w = window;
  if (!points.length) return;
  // Thin and translucent, drawn on at the finger's pace, and faded as soon as the finger lifts: the ink underneath is what
  // a screenshot must show. The number stays beside the start for a moment so a frame says which step drew it.
  const svg = overlay("wf-browser-stroke-indicator", fadeMs);
  const d = points.map((p) => `${p.x},${p.y}`).join(" ");
  const line = svgEl("polyline", { points: d, fill: "none", stroke: color, "stroke-width": 2.5, "stroke-opacity": 0.6, "stroke-linecap": "round", "stroke-linejoin": "round" });
  svg.appendChild(line);
  const first = points[0];
  const sameAction = w.__mcpLastStrokeN === n;
  w.__mcpLastStrokeN = n;
  if (!sameAction) badge(svg, first.x, first.y, n, color);
  document.body.appendChild(svg);
  const length = line.getTotalLength ? line.getTotalLength() : 0;
  if (length > 0) {
    line.style.strokeDasharray = String(length);
    line.animate([{ strokeDashoffset: length }, { strokeDashoffset: 0 }], { duration: durationMs, easing: "linear", fill: "forwards" });
  }
  const removeNow = keep(w, "__mcpStrokeMarkers", svg, max);
  setTimeout(() => {
    line.style.transition = `opacity ${fadeMs}ms ease-out`;
    line.style.opacity = "0";
    // the badge outlives the line, then the whole marker goes
    setTimeout(() => {
      svg.style.transition = `opacity ${fadeMs}ms ease-in`;
      svg.style.opacity = "0";
      setTimeout(removeNow, fadeMs);
    }, Math.max(lingerMs, 1200));
  }, durationMs);
};

// A key of the mock keyboard lights up as a finger would press it: a tinted pulse that settles back.
const keyTint = ({ key, color, max, lingerMs, fadeMs }) => {
  const w = window;
  const btn = document.querySelector(`.mock-keyboard [data-mcp-key="${CSS.escape(key)}"]`);
  if (!btn) return;
  w.__mcpKeyTints = w.__mcpKeyTints ?? [];
  const tints = w.__mcpKeyTints;
  const scheduleFade = (entry2) => {
    clearTimeout(entry2.timer);
    entry2.timer = setTimeout(() => {
      entry2.el.style.transition = `background-color ${fadeMs}ms ease-in, box-shadow ${fadeMs}ms ease-in`;
      entry2.el.style.background = entry2.prevBackground;
      entry2.el.style.boxShadow = entry2.prevShadow;
      setTimeout(() => {
        entry2.el.style.transition = entry2.prevTransition;
        const i = tints.indexOf(entry2);
        if (i !== -1) tints.splice(i, 1);
      }, fadeMs);
    }, lingerMs);
  };
  const existingIdx = tints.findIndex((e) => e.el === btn);
  if (existingIdx !== -1) {
    const entry2 = tints.splice(existingIdx, 1)[0];
    if (entry2) {
      tints.push(entry2);
      btn.animate([{ transform: "scale(1)" }, { transform: "scale(0.92)" }, { transform: "scale(1)" }], { duration: 160 });
      scheduleFade(entry2);
    }
    return;
  }
  const entry = { el: btn, prevBackground: btn.style.background, prevShadow: btn.style.boxShadow, prevTransition: btn.style.transition, timer: 0 };
  btn.style.transition = "";
  btn.style.background = color;
  btn.style.boxShadow = `0 0 0 3px ${color}55`;
  btn.animate([{ transform: "scale(1)" }, { transform: "scale(0.92)" }, { transform: "scale(1)" }], { duration: 160 });
  tints.push(entry);
  scheduleFade(entry);
  while (tints.length > max) {
    const oldest = tints.shift();
    if (oldest) {
      clearTimeout(oldest.timer);
      oldest.el.style.background = oldest.prevBackground;
      oldest.el.style.boxShadow = oldest.prevShadow;
    }
  }
};

// The field being typed into gets a soft focus ring for as long as keys arrive.
const typeFlash = (el, { color, lingerMs, fadeMs }) => {
  const target = el;
  if (target.__mcpFlashTimer === void 0) {
    target.__mcpFlashPrev = target.style.boxShadow;
  } else {
    clearTimeout(target.__mcpFlashTimer);
  }
  target.style.transition = "";
  target.style.boxShadow = `inset 0 0 0 2px ${color}, 0 0 0 4px ${color}33`;
  target.__mcpFlashTimer = setTimeout(() => {
    target.style.transition = `box-shadow ${fadeMs}ms ease-in`;
    target.style.boxShadow = target.__mcpFlashPrev ?? "";
    setTimeout(() => {
      target.style.transition = "";
    }, fadeMs);
    delete target.__mcpFlashTimer;
  }, lingerMs);
};
return { tap, drag, keyTint, typeFlash, stroke };
})()
