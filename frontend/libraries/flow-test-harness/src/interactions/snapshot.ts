// Golden snapshot writer: freezes the current DOM (head styles, viewport meta,
// laid-out boxes) into a standalone HTML file a headless Chromium pass renders.
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { expect } from 'vitest';

import type { LayoutBox } from './layout';

type Box = LayoutBox;

export type SnapshotPageOptions = {
  suite?: string;
  outDir?: string;
  layouts: ReadonlyMap<Element, Box>;
};

const SLUG_RE = /[^a-z0-9-]+/g;

let written: string[] = [];

/** The snapshot files written since the last call; the setup file files them under the test for the flow report. */
export function takeWrittenSnapshots(): string[] {
  const out = written;
  written = [];
  return out;
}

function slug(input: string): string {
  return input
    .toLowerCase()
    .replace(SLUG_RE, '-')
    .replace(/-{2,}/g, '-')
    .replace(/^-|-$/g, '');
}

function defaultSuite(): string {
  const testPath = expect.getState().testPath;
  const basename = testPath ? path.basename(testPath) : '';
  const stripped = basename
    .replace(/\.flow\.test\.(tsx|ts)$/i, '')
    .replace(/\.test\.(tsx|ts)$/i, '');
  return slug(stripped || 'flow');
}

function defaultOutDir(): string {
  return process.env.FLOW_GOLDENS_OUT ?? path.join(process.cwd(), '.flow-goldens', 'snapshots');
}

// Tamagui and react-native-web insert rules through the CSSOM, so a <style> element's text is often empty; serialise the live rules.
function headStyles(doc: Document = document): string[] {
  return Array.from(doc.styleSheets).map((sheet) => {
    let rules = '';
    try {
      rules = Array.from(sheet.cssRules, (rule) => rule.cssText).join('\n');
    } catch {
      rules = sheet.ownerNode?.textContent ?? '';
    }
    return `<style>${rules}</style>`;
  });
}

// Serialize body with a copy of the live tree so the stored layout boxes can be
// injected as inline width/height without mutating the DOM the test asserts on.
function injectLayoutBoxes(
  root: HTMLElement,
  layouts: ReadonlyMap<Element, Box>
): string {
  const clone = root.cloneNode(true) as HTMLElement;
  const walk = (live: Element, copy: Element): void => {
    // A frame's page ran scripts the frozen copy will not; freeze its current DOM in place of its source.
    if (live instanceof HTMLIFrameElement && live.contentDocument?.body) {
      const body = live.contentDocument.body.cloneNode(true) as HTMLElement;
      body.querySelectorAll('script').forEach((script) => script.remove());
      copy.setAttribute(
        'srcdoc',
        ['<!doctype html><meta charset="utf-8">', ...headStyles(live.contentDocument), body.outerHTML].join('\n')
      );
      copy.removeAttribute('src');
    }
    const box = layouts.get(live);
    if (box) {
      (copy as HTMLElement).style.width = `${box.width}px`;
      (copy as HTMLElement).style.height = `${box.height}px`;
    }
    const count = Math.min(live.children.length, copy.children.length);
    for (let i = 0; i < count; i++) walk(live.children[i], copy.children[i]);
  };
  walk(root, clone);
  return clone.innerHTML;
}

export function snapshotPage(
  name: string,
  opts: SnapshotPageOptions
): string {
  const suite = opts.suite ?? defaultSuite();
  const outDir = opts.outDir ?? defaultOutDir();
  const dir = path.join(outDir, slug(suite));
  mkdirSync(dir, { recursive: true });
  const file = path.join(dir, `${slug(name)}.html`);
  const html = [
    '<!doctype html>',
    '<meta charset="utf-8">',
    '<meta name="viewport" content="width=device-width, initial-scale=1">',
    ...headStyles(),
    injectLayoutBoxes(document.body, opts.layouts),
  ].join('\n');
  writeFileSync(file, html, 'utf8');
  written.push(file);
  return file;
}