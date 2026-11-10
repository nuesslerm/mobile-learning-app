// features/coding-agent/src/domain/diff/parseUnifiedDiff.ts
//
// Pure parser for unified-diff text. Returns the shape the contract promises:
// `ParsedDiffFile[] {path, oldPath?, additions, deletions, hunks: {header,
// lines: {kind: add|del|ctx, text, oldNo?, newNo?}[]}[]}`. Recognises
// `diff --git a/X b/Y` file headers, the `---`/`+++` path lines (including
// `/dev/null` for added/removed files), `@@ -a,b +c,d @@` hunk headers, and
// the per-line markers (`+`, `-`, ` `). Renames (`rename from` / `rename to`
// inside a `diff --git`) keep `oldPath`. Binary files surface as a file
// entry with an empty `hunks` array and zero counts. Malformed hunk headers
// abort the current file rather than throwing — the next `diff --git` still
// parses cleanly.

type ParsedDiffLineKind = 'add' | 'del' | 'ctx';

interface ParsedDiffLine {
  kind: ParsedDiffLineKind;
  text: string;
  oldNo?: number;
  newNo?: number;
}

interface ParsedDiffHunk {
  header: string;
  lines: ParsedDiffLine[];
}

export interface ParsedDiffFile {
  path: string;
  oldPath?: string;
  additions: number;
  deletions: number;
  hunks: ParsedDiffHunk[];
}

const HUNK_HEADER = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/;
const GIT_HEADER = /^diff --git a\/(.+) b\/(.+)$/;
const RENAME_FROM = /^rename from (.+)$/;
const RENAME_TO = /^rename to (.+)$/;

function stripGitPrefix(rawPath: string): string {
  return rawPath.replace(/^a\//, '').replace(/^b\//, '');
}

function pushFile(
  files: ParsedDiffFile[],
  current: ParsedDiffFile | null
): void {
  if (current === null) {
    return;
  }
  if (
    current.path.length > 0 ||
    current.oldPath !== undefined ||
    current.hunks.length > 0
  ) {
    files.push(current);
  }
}

export function parseUnifiedDiff(text: string): ParsedDiffFile[] {
  const lines = text.replace(/\r\n/g, '\n').split('\n');
  const files: ParsedDiffFile[] = [];

  let current: ParsedDiffFile | null = null;
  let hunk: ParsedDiffHunk | null = null;
  let oldNo = 0;
  let newNo = 0;
  let skipUntilNextFile = false;
  let fileIsSkipped = false;

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index] ?? '';

    if (line.startsWith('diff --git ')) {
      if (!fileIsSkipped) {
        pushFile(files, current);
      }
      const match = GIT_HEADER.exec(line);
      const oldPath = match ? stripGitPrefix(match[1] ?? '') : '';
      const newPath = match ? stripGitPrefix(match[2] ?? '') : '';
      current = {
        path: newPath,
        ...(oldPath.length > 0 ? { oldPath } : {}),
        additions: 0,
        deletions: 0,
        hunks: [],
      };
      hunk = null;
      oldNo = 0;
      newNo = 0;
      skipUntilNextFile = false;
      fileIsSkipped = false;
      continue;
    }

    if (current === null) {
      continue;
    }

    if (line.startsWith('Binary files ')) {
      pushFile(files, current);
      current = null;
      hunk = null;
      skipUntilNextFile = false;
      fileIsSkipped = false;
      continue;
    }

    const renameFromMatch = RENAME_FROM.exec(line);
    if (renameFromMatch) {
      const renamed = stripGitPrefix(renameFromMatch[1] ?? '');
      if (renamed.length > 0) {
        current.oldPath = renamed;
      }
      continue;
    }

    const renameToMatch = RENAME_TO.exec(line);
    if (renameToMatch) {
      const renamed = stripGitPrefix(renameToMatch[1] ?? '');
      if (renamed.length > 0) {
        current.path = renamed;
      }
      continue;
    }

    if (line.startsWith('--- ')) {
      const trimmed = line.slice(4).trim();
      if (trimmed === '/dev/null') {
        delete current.oldPath;
      } else {
        const oldPath = stripGitPrefix(trimmed);
        if (oldPath.length > 0) {
          current.oldPath = oldPath;
          if (current.path.length === 0) {
            current.path = oldPath;
          }
        }
      }
      continue;
    }

    if (line.startsWith('+++ ')) {
      const trimmed = line.slice(4).trim();
      if (trimmed !== '/dev/null') {
        const newPath = stripGitPrefix(trimmed);
        if (newPath.length > 0) {
          current.path = newPath;
        }
      }
      continue;
    }

    if (skipUntilNextFile) {
      continue;
    }

    if (line.startsWith('@@') && !HUNK_HEADER.test(line)) {
      skipUntilNextFile = true;
      fileIsSkipped = true;
      hunk = null;
      continue;
    }

    const hunkMatch = HUNK_HEADER.exec(line);
    if (hunkMatch) {
      const oldStart = Number.parseInt(hunkMatch[1] ?? '0', 10);
      const oldLenRaw = hunkMatch[2];
      const newStart = Number.parseInt(hunkMatch[3] ?? '0', 10);
      const newLenRaw = hunkMatch[4];
      const oldLen =
        oldLenRaw === undefined ? 1 : Number.parseInt(oldLenRaw, 10);
      const newLen =
        newLenRaw === undefined ? 1 : Number.parseInt(newLenRaw, 10);
      if (
        !Number.isFinite(oldStart) ||
        !Number.isFinite(oldLen) ||
        !Number.isFinite(newStart) ||
        !Number.isFinite(newLen) ||
        oldLen < 0 ||
        newLen < 0
      ) {
        skipUntilNextFile = true;
        fileIsSkipped = true;
        hunk = null;
        continue;
      }
      hunk = { header: line, lines: [] };
      current.hunks.push(hunk);
      oldNo = oldStart;
      newNo = newStart;
      let oldRemaining = oldLen;
      let newRemaining = newLen;
      let bodyRead = 0;
      while (oldRemaining > 0 || newRemaining > 0) {
        const bodyLine = lines[index + 1 + bodyRead];
        if (bodyLine === undefined) {
          break;
        }
        const marker = bodyLine[0];
        const text = bodyLine.length > 0 ? bodyLine.slice(1) : '';
        if (marker === '+') {
          hunk.lines.push({ kind: 'add', text, newNo });
          newNo += 1;
          newRemaining -= 1;
        } else if (marker === '-') {
          hunk.lines.push({ kind: 'del', text, oldNo });
          oldNo += 1;
          oldRemaining -= 1;
        } else if (marker === ' ' || marker === undefined) {
          hunk.lines.push({ kind: 'ctx', text, oldNo, newNo });
          oldNo += 1;
          newNo += 1;
          oldRemaining -= 1;
          newRemaining -= 1;
        } else {
          break;
        }
        bodyRead += 1;
      }
      index += bodyRead;
      current.additions += hunk.lines.filter((l) => l.kind === 'add').length;
      current.deletions += hunk.lines.filter((l) => l.kind === 'del').length;
      continue;
    }

    if (line.startsWith('\\')) {
      continue;
    }
  }

  pushFile(files, current);
  return files;
}
