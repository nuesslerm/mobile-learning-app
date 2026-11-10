/**
 * ChartSpike gate (agent-dashboards, unit c1/c2): prove victory-native actually
 * DREW on the web export, not merely that the spike's testID exists.
 *
 * The vacuous pass this guards against is a present ChartSpike subtree with an
 * empty canvas inside it: CanvasKit is an ~8MB WASM loaded from a CDN, so
 * "the route rendered" and "the charts painted" are two different facts, and a
 * check that stopped at testID presence would stay green through the exact
 * failure -- Skia never initialising, or the canvas sizing to zero -- that the
 * plan's first unit exists to rule out. So the assertion is on geometry: at
 * least one canvas/svg inside a chart wrapper with a non-zero measured width
 * and height, read out of the live DOM of the served export.
 */
import { fail, withServedApp } from './lib/servedApp.mjs';

const CANVAS_TIMEOUT_MS = 90000;
const POLL_MS = 500;

await withServedApp(
  { port: 8222, name: 'charts-spike-verify', path: 'charts-spike' },
  async ({ evalInApp }) => {
    const probe = `(() => {
      const root = appDoc.querySelector('[data-testid="ChartSpike"]');
      if (!root) return { ok: false, phase: 'no ChartSpike root' };
      const measures = [];
      const wrappers = {};
      for (const id of ['ChartSpikeTokensCanvas', 'ChartSpikeCpuCanvas']) {
        const host = appDoc.querySelector('[data-testid="' + id + '"]');
        wrappers[id] = !!host;
        if (!host) continue;
        for (const el of host.querySelectorAll('canvas, svg')) {
          const r = el.getBoundingClientRect();
          measures.push({
            host: id,
            tag: el.tagName.toLowerCase(),
            w: Math.round(r.width),
            h: Math.round(r.height),
          });
        }
      }
      const drawn = measures.filter((m) => m.w > 0 && m.h > 0);
      return {
        ok: drawn.length > 0,
        measures,
        drawn,
        wrappers,
        loading: !!appDoc.querySelector('[data-testid="ChartSpikeLoading"]'),
        pageErrors: (appDoc.defaultView.__pageErrors || []).slice(0, 3),
        text: appDoc.body.innerText.slice(0, 120),
      };
    })()`;

    const deadline = Date.now() + CANVAS_TIMEOUT_MS;
    let last = null;
    while (Date.now() < deadline) {
      last = await evalInApp(probe);
      if (last?.ok) break;
      await new Promise((r) => setTimeout(r, POLL_MS));
    }

    if (!last?.ok) {
      fail(`ChartSpike did not draw a canvas/svg with size: ${JSON.stringify(last)}`);
      return;
    }
    const drawn = last.drawn.map((m) => `${m.tag} ${m.w}x${m.h}`).join(', ');
    console.log(`ok: chart drew — ${drawn}`);
  }
);