// Adapted from Orca mobile at 2c0cbbc (MIT, Lovecast Inc.).
import { common, createLowlight } from 'lowlight';

export type TokenKind =
  | 'plain'
  | 'comment'
  | 'keyword'
  | 'string'
  | 'number'
  | 'type'
  | 'function'
  | 'variable'
  | 'meta'
  | 'tag';

export type Segment = {
  text: string;
  kind: TokenKind;
};

type LowlightNode = {
  type: string;
  value?: string;
  properties?: {
    className?: unknown;
  };
  children?: LowlightNode[];
};

const lowlight = createLowlight(common);
const MAX_FILE_HIGHLIGHT_CHARS = 48_000;
const MAX_FILE_HIGHLIGHT_SEGMENTS = 3_000;
// A Map, not an object: fence tags are agent text, and `constructor` must not resolve.
const LANGUAGE_ALIASES = new Map([
  ['javascriptreact', 'javascript'],
  ['jsx', 'javascript'],
  ['typescriptreact', 'typescript'],
  ['tsx', 'typescript'],
  ['sh', 'bash'],
  ['shell', 'bash'],
  ['shellscript', 'bash'],
  ['zsh', 'bash'],
  ['fish', 'bash'],
  ['yml', 'yaml'],
  ['cuda-cpp', 'cpp'],
  ['glimmer-js', 'javascript'],
  ['glimmer-ts', 'typescript'],
  // lowlight has no dotenv grammar; ini covers KEY=value and # comments.
  ['dotenv', 'ini'],
  // HCL assignments, strings and comments use the bundled ini grammar on mobile.
  ['hcl', 'ini'],
]);

export function highlightCode(
  code: string,
  language: string,
  maxHighlightChars = MAX_FILE_HIGHLIGHT_CHARS,
  maxHighlightSegments = MAX_FILE_HIGHLIGHT_SEGMENTS
): { segments: Segment[]; highlighted: boolean } {
  if (code.length === 0) {
    return { segments: [{ text: '', kind: 'plain' }], highlighted: false };
  }

  const normalizedLanguage = LANGUAGE_ALIASES.get(language) ?? language;
  if (
    !lowlight.registered(normalizedLanguage) ||
    normalizedLanguage === 'plaintext'
  ) {
    return { segments: [{ text: code, kind: 'plain' }], highlighted: false };
  }

  const highlightLength = getHighlightBoundary(code, maxHighlightChars);
  const highlightedCode = code.slice(0, highlightLength);
  try {
    const tree = lowlight.highlight(
      normalizedLanguage,
      highlightedCode
    ) as LowlightNode;
    const segments = flattenLowlightNodes(tree.children ?? [], 'plain');
    if (segments.length > maxHighlightSegments) {
      return { segments: [{ text: code, kind: 'plain' }], highlighted: false };
    }
    if (highlightLength < code.length) {
      appendSegment(segments, {
        text: code.slice(highlightLength),
        kind: 'plain',
      });
    }
    return { segments, highlighted: true };
  } catch {
    return { segments: [{ text: code, kind: 'plain' }], highlighted: false };
  }
}

function getHighlightBoundary(code: string, maxHighlightChars: number): number {
  if (code.length <= maxHighlightChars) {
    return code.length;
  }
  const boundary = code.lastIndexOf('\n', maxHighlightChars);
  return boundary > 0 ? boundary + 1 : maxHighlightChars;
}

function flattenLowlightNodes(
  nodes: LowlightNode[],
  inheritedKind: TokenKind
): Segment[] {
  const segments: Segment[] = [];
  for (const node of nodes) {
    if (node.type === 'text') {
      appendSegment(segments, { text: node.value ?? '', kind: inheritedKind });
      continue;
    }
    if (node.type !== 'element') {
      continue;
    }
    const kind =
      tokenKindForClasses(node.properties?.className) ?? inheritedKind;
    for (const segment of flattenLowlightNodes(node.children ?? [], kind)) {
      appendSegment(segments, segment);
    }
  }
  return segments;
}

function tokenKindForClasses(className: unknown): TokenKind | null {
  const classes = Array.isArray(className)
    ? className.filter((value): value is string => typeof value === 'string')
    : typeof className === 'string'
      ? className.split(/\s+/)
      : [];

  const tokens = new Set(classes.map((value) => value.replace(/^hljs-/, '')));
  // Buckets follow highlight.js's GitHub theme, except `attr` (JSON/YAML keys) is green as on github.com.
  if (tokens.has('subst')) {
    return 'plain';
  }
  if (hasAny(tokens, ['comment', 'quote'])) {
    return 'comment';
  }
  if (hasAny(tokens, ['keyword', 'doctag', 'template-tag', 'language_'])) {
    return 'keyword';
  }
  if (hasAny(tokens, ['name', 'selector-tag', 'selector-pseudo', 'attr'])) {
    return 'tag';
  }
  if (hasAny(tokens, ['string', 'regexp', 'symbol', 'bullet'])) {
    return 'string';
  }
  if (
    hasAny(tokens, [
      'number',
      'literal',
      'attribute',
      'selector-class',
      'selector-id',
    ])
  ) {
    return 'number';
  }
  if (hasAny(tokens, ['type', 'built_in'])) {
    return 'type';
  }
  if (hasAny(tokens, ['title', 'function'])) {
    return 'function';
  }
  if (hasAny(tokens, ['variable', 'template-variable'])) {
    return 'variable';
  }
  if (hasAny(tokens, ['meta', 'section'])) {
    return 'meta';
  }
  return null;
}

function hasAny(values: Set<string>, candidates: string[]): boolean {
  return candidates.some((candidate) => values.has(candidate));
}

function appendSegment(segments: Segment[], segment: Segment): void {
  if (!segment.text) {
    return;
  }
  const previous = segments.at(-1);
  if (previous?.kind === segment.kind) {
    previous.text += segment.text;
    return;
  }
  segments.push({ ...segment });
}
