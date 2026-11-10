/**
 * Draws every generated letter -- outline filled, centre line on top, strokes
 * numbered in order -- so the extraction can be LOOKED at.
 *
 * Run: `pnpm --filter mla-tamagui run glyphs:preview`
 * Writes an SVG and a PNG next to each other in .agents/tmp/.
 *
 * The measurements in `extract-tamil-glyphs.ts` say a stroke is on the letter
 * and the letter is covered. They cannot say the stroke ORDER is sane, or that
 * a stroke runs the way a hand would. That still needs eyes, so this makes
 * looking cheap.
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { createContainer } from '@lib/browser-client';

import { GENERATED_GLYPHS as ALL_GLYPHS } from '../features/tamil-writing/domain/glyphs.generated';
import type { Point } from '../features/tamil-writing/domain/strokes';

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT_DIR = resolve(HERE, '..', '..', '..', '.agents', 'tmp');
const SVG = join(OUT_DIR, 'tamil-glyph-preview.svg');

const CELL = 200;
const COLUMNS = 4;
/** `--only <substring>` narrows the sheet to the letters worth looking at. */
const ONLY = process.argv.includes('--only')
  ? process.argv[process.argv.indexOf('--only') + 1]
  : undefined;
const COLORS = [
  '#e5484d',
  '#0091ff',
  '#30a46c',
  '#f5a524',
  '#8e4ec6',
  '#d6409f',
  '#12a594',
  '#e93d82',
];

function at(points: readonly Point[], index: number): Point {
  const point = points[index];
  if (!point) throw new Error(`no point at ${index}`);
  return point;
}

function scale(point: Point): string {
  return `${(point.x * CELL).toFixed(1)} ${(point.y * CELL).toFixed(1)}`;
}

const GENERATED_GLYPHS = ONLY
  ? ALL_GLYPHS.filter((glyph) => glyph.characterId.includes(ONLY))
  : ALL_GLYPHS;

const cells = GENERATED_GLYPHS.map((glyph, index) => {
  const x = (index % COLUMNS) * CELL;
  const y = Math.floor(index / COLUMNS) * CELL;
  const outline = glyph.contours
    .map((contour) => `M ${contour.map(scale).join(' L ')} Z`)
    .join(' ');

  const strokes = glyph.strokes
    .map((stroke, order) => {
      const color = COLORS[order % COLORS.length] ?? '#000';
      const start = at(stroke, 0);
      const end = at(stroke, stroke.length - 1);
      return [
        `<path d="M ${stroke.map(scale).join(' L ')}" stroke="${color}" stroke-width="3" fill="none" stroke-linecap="round"/>`,
        `<circle cx="${(start.x * CELL).toFixed(1)}" cy="${(start.y * CELL).toFixed(1)}" r="4" fill="${color}"/>`,
        `<circle cx="${(end.x * CELL).toFixed(1)}" cy="${(end.y * CELL).toFixed(1)}" r="2.5" fill="none" stroke="${color}" stroke-width="2"/>`,
        `<text x="${(start.x * CELL + 6).toFixed(1)}" y="${(start.y * CELL - 4).toFixed(1)}" font-size="11" fill="${color}">${order + 1}</text>`,
      ].join('\n      ');
    })
    .join('\n      ');

  return `<g transform="translate(${x},${y})">
      <rect width="${CELL}" height="${CELL}" fill="#fff" stroke="#ddd"/>
      <path d="${outline}" fill="#cfcfcf" fill-rule="nonzero"/>
      ${strokes}
      <text x="6" y="14" font-size="11" fill="#666">${glyph.characterId} · ${glyph.strokes.length} strokes</text>
    </g>`;
}).join('\n');

const rows = Math.ceil(GENERATED_GLYPHS.length / COLUMNS);
mkdirSync(OUT_DIR, { recursive: true });
writeFileSync(
  SVG,
  `<svg xmlns="http://www.w3.org/2000/svg" width="${COLUMNS * CELL}" height="${rows * CELL}" viewBox="0 0 ${COLUMNS * CELL} ${rows * CELL}"><rect width="100%" height="100%" fill="#fafafa"/>
${cells}
</svg>`,
  'utf8'
);
console.log(`wrote ${SVG}`);

// Wrapped rather than top-level await: tsx transpiles this file to CJS.
async function capture(): Promise<void> {
  const container = createContainer({ headless: true, screenshotDir: OUT_DIR });
  // No executable path: the daemon falls back to /opt/pw-browsers/chromium on
  // its own when the pinned revision is missing, and the option is not part of
  // its typed surface.
  await container.startDaemon({});
  await container.openBrowser({ name: 'tamil-glyphs', renderMode: 'plain' });
  await container.navigate({ name: 'tamil-glyphs', url: `file://${SVG}` });
  await new Promise((done) => setTimeout(done, 800));
  const shot = await container.screenshot({ name: 'tamil-glyphs' });
  console.log(`wrote ${shot?.path ?? '(no screenshot)'}`);
  await container.stopDaemon();
}

void capture();
