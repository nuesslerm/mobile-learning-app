import type { Annotation } from '../domain/types';

/**
 * Render captured annotations into agent-ready markdown — the same idea as the
 * web `agentation` tool, but keyed on the real `file:line` (from babel
 * jsx-source) instead of an inferred CSS selector.
 *
 * Pure function: deterministic, no IO. Unit-tested in tests/toMarkdown.test.ts.
 */
export function toMarkdown(annotations: readonly Annotation[]): string {
  if (annotations.length === 0) {
    return '# Agentation\n\n_No annotations captured yet._\n';
  }

  const lines: string[] = ['# Agentation feedback', ''];

  annotations.forEach((a, i) => {
    lines.push(`## ${i + 1}. ${a.componentName}`);
    lines.push('');
    if (a.source) {
      const col =
        a.source.columnNumber != null ? `:${a.source.columnNumber}` : '';
      // `file:line` — clickable in editors / greppable by the agent.
      lines.push(`- **Source:** \`${a.source.fileName}:${a.source.lineNumber}${col}\``);
    } else if (a.sourceFile) {
      // React 19 drops the line but keeps the file in the dev display name.
      lines.push(`- **Source file:** \`${a.sourceFile}\``);
    } else {
      lines.push(
        '- **Source:** _unavailable — use the component chain below to locate it_',
      );
    }
    if (a.ownerChain && a.ownerChain.length > 0) {
      lines.push(`- **Component chain:** ${a.ownerChain.join(' › ')}`);
    }
    lines.push(`- **Platform:** ${a.platform}`);
    if (a.rect) {
      const { x, y, width, height } = a.rect;
      lines.push(
        `- **Bounds:** x=${round(x)} y=${round(y)} w=${round(width)} h=${round(height)}`,
      );
    }
    if (a.note.trim().length > 0) {
      lines.push(`- **Note:** ${a.note.trim()}`);
    }
    const propsBlock = formatProps(a.props);
    if (propsBlock) {
      lines.push('- **Props:**');
      lines.push('');
      lines.push('```json');
      lines.push(propsBlock);
      lines.push('```');
    }
    lines.push('');
  });

  return lines.join('\n');
}

function round(n: number): number {
  return Math.round(n);
}

/** JSON-stringify props defensively; drops non-serializable values. */
function formatProps(props: Record<string, unknown> | undefined): string | null {
  if (!props || Object.keys(props).length === 0) {
    return null;
  }
  try {
    return JSON.stringify(props, safeReplacer, 2);
  } catch {
    return null;
  }
}

function safeReplacer(_key: string, value: unknown): unknown {
  if (typeof value === 'function') {
    return '[Function]';
  }
  if (typeof value === 'symbol') {
    return value.toString();
  }
  return value;
}
