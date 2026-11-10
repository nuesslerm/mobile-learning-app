import type { Annotation } from '../domain/types';
import { ratioToPercent, verticalGap, type Viewport } from '../geometry/wireframeGeometry';

/**
 * Render wireframe placement annotations into the upstream-style
 * markdown layout report.
 *
 * Pure function: deterministic, no IO.
 */
export function toWireframeMarkdown(
  annotations: readonly Annotation[],
  pageTitle = '/wireframe',
  viewport: Viewport = { width: 1728, height: 963 },
): string {
  const placements = annotations.filter(
    (a): a is Annotation & { wireframe: NonNullable<Annotation['wireframe']> } =>
      a.wireframe != null,
  );

  if (placements.length === 0) {
    return '';
  }

  const sorted = [...placements].sort((a, b) => a.wireframe.yRatio - b.wireframe.yRatio);
  const lines: string[] = [
    '## Page Feedback: ' + pageTitle,
    '',
    '## Design Layout',
    '',
    `> ${sorted.length} component${sorted.length === 1 ? '' : 's'} placed`,
    '',
    '### Reference Frame',
    `- Viewport: \`${viewport.width}×${viewport.height}px\``,
    '- Content area: 100% width (relative to viewport)',
    '- Vertical gap between elements: `next.y - (prev.y + prev.height)` → `margin-top`',
    '',
    '### Components',
  ];

  sorted.forEach((a, i) => {
    const w = a.wireframe;
    const xPx = Math.round(w.xRatio * viewport.width);
    const yPx = Math.round(w.yRatio * viewport.height);
    const wPx = Math.round(w.widthRatio * viewport.width);
    const hPx = Math.round(w.heightRatio * viewport.height);
    const prev = sorted[i - 1]?.wireframe;
    const gap = prev ? verticalGap(prev, w, viewport.height) : null;

    lines.push(
      `${i + 1}. **${w.componentType}** — \`${wPx}×${hPx}px\` at \`(${xPx}, ${yPx})\``,
    );
    if (gap != null && prev) {
      lines.push(`   - Below **${prev.componentType}** (${gap}px gap)`);
    }
    lines.push(`   - \`width: ${ratioToPercent(w.widthRatio)}\``);
    lines.push(`   - \`left: ${ratioToPercent(w.xRatio)}\``);
    if (a.note.trim()) {
      lines.push(`   - Note: ${a.note.trim()}`);
    }
  });

  lines.push('', '### Layout Analysis');
  const rows: Record<string, string[]> = {};
  sorted.forEach((a) => {
    const key = `y≈${Math.round(a.wireframe.yRatio * viewport.height)}`;
    (rows[key] ??= []).push(a.wireframe.componentType);
  });
  Object.entries(rows).forEach(([y, comps]) => {
    lines.push(`- Row ${y}: ${comps.join(', ')}`);
  });

  lines.push('', '### Suggested Implementation');
  lines.push('');
  sorted.forEach((a) => {
    const w = a.wireframe;
    lines.push(`- \`${w.componentType}\` — \`width: ${ratioToPercent(w.widthRatio)}; left: ${ratioToPercent(w.xRatio)}; top: ${ratioToPercent(w.yRatio)}\``);
  });
  lines.push('');

  return lines.join('\n');
}
