import { useRef, useState } from 'react';

import type { SourceCodeLine, SourceControlProps, SourceFile, SourceFileReview } from '../ports/SourceControl';

type Panel = SourceControlProps['prPanel'];
export type SourcePullRequest = NonNullable<NonNullable<Panel['result']>['pullRequest']>;

/** How long ago a commit was written, in the one unit that fits. */
export function commitAge(timestamp: number): string {
  const minutes = Math.floor((Date.now() - timestamp) / 60_000);
  if (minutes < 1) return 'just now';
  if (minutes < 60) return minutes + 'm';
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return hours + 'h';
  const days = Math.floor(hours / 24);
  if (days < 30) return days + 'd';
  const months = Math.floor(days / 30);
  return months < 12 ? months + 'mo' : Math.floor(months / 12) + 'y';
}

/** What the screen's bar says on each of its pages, and where its back button leads. */
export function sourceControlHeading(props: Pick<SourceControlProps, 'page' | 'selected' | 'branch' | 'head' | 'backLabel'>) {
  const { page } = props;
  const branch = props.branch ?? props.head?.slice(0, 7);
  return {
    branch,
    title: page === 'changes' ? 'Source Control' : page === 'commits' ? 'Commits' : 'Commit',
    subtitle: page === 'detail' ? props.selected?.shortSha : branch,
    backLabel: page === 'detail' ? 'Back to commits' : page === 'commits' ? 'Back to source control' : (props.backLabel ?? 'Back to workspaces'),
  };
}

export function fileTotals(files: readonly SourceFile[] | undefined) {
  return files?.reduce((sum, file) => ({ added: sum.added + (file.additions ?? 0), removed: sum.removed + (file.deletions ?? 0) }), { added: 0, removed: 0 });
}

/** A file's one-letter status, as git prints it. */
export function fileStatusLetter(file: SourceFile): string | undefined {
  return file.status === 'type_changed' ? 'T' : file.status[0]?.toUpperCase();
}

/** Linking an existing pull request by its number or address: the field, one request at a time, and what a refusal says. */
export function useLinkPullRequest(panel: Panel, done: () => void) {
  const [input, setInput] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const pending = useRef(false);
  const number = panel.parseReference(input);
  function submit(): Promise<void> {
    if (!number || pending.current) return Promise.resolve();
    pending.current = true;
    setBusy(true);
    setError('');
    const settle = () => {
      pending.current = false;
      setBusy(false);
    };
    return panel.link(number).then(
      () => {
        settle();
        done();
      },
      () => {
        settle();
        setError('Could not link this pull request. Check the number and try again.');
      }
    );
  }
  return { input, setInput, busy, error, number, submit, submitLabel: number ? 'Link #' + number : 'Link pull request' };
}

const plural = (count: number, one: string, many: string) => count + ' ' + (count === 1 ? one : many);

/** A pull request in the words the screen shows: its state, its checks, its review and whether it merges. */
export function pullRequestFacts(pr: SourcePullRequest) {
  const { checks } = pr;
  return {
    state: pr.state === 'open' ? (pr.draft ? 'Draft' : 'Open') : pr.state === 'merged' ? 'Merged' : 'Closed',
    tone: pr.state === 'merged' ? ('merged' as const) : pr.state === 'closed' ? ('closed' as const) : pr.draft ? ('draft' as const) : ('open' as const),
    checks: !checks
      ? 'Checks unavailable'
      : checks.failing
        ? plural(checks.failing, 'check failed', 'checks failed')
        : checks.pending
          ? plural(checks.pending, 'check running', 'checks running')
          : checks.passing
            ? plural(checks.passing, 'check passed', 'checks passed')
            : 'No checks reported',
    checksTone: checks?.failing ? ('failing' as const) : checks?.pending ? ('pending' as const) : ('quiet' as const),
    review: !pr.reviewDecision ? undefined : pr.reviewDecision === 'approved' ? 'Approved' : pr.reviewDecision === 'changes_requested' ? 'Changes requested' : 'Review required',
    merge: pr.mergeable === 'mergeable' ? 'No conflicts with ' + pr.base : pr.mergeable === 'conflicting' ? 'Conflicts with ' + pr.base : 'Mergeability unknown',
    filesChanged: pr.files === null ? undefined : plural(pr.files, 'file', 'files') + ' changed',
  };
}

export type FoldedRow = { line: SourceCodeLine; index: number } | { fold: number; index: number; scope?: string };

/** A diff's lines with each long run of unchanged ones folded to three either side, a fold named after the block it sits in. */
export function foldedRows(lines: SourceCodeLine[], expanded: ReadonlySet<number>): FoldedRow[] {
  const rows: FoldedRow[] = [];
  for (let start = 0; start < lines.length; ) {
    const line = lines[start]!;
    if (line.kind !== 'context') {
      rows.push({ index: start, line });
      start++;
      continue;
    }
    let end = start + 1;
    while (end < lines.length && lines[end]?.kind === 'context') end++;
    const from = start === 0 ? start : start + 3;
    const to = end === lines.length ? end : end - 3;
    for (let i = start; i < end; i++) {
      if (i === from && to - from >= 3 && !expanded.has(from)) {
        rows.push({ index: i, fold: to - from });
        i = to - 1;
      } else rows.push({ index: i, line: lines[i]! });
    }
    start = end;
  }
  const folds = rows.filter((row): row is Extract<FoldedRow, { fold: number }> => 'fold' in row);
  const scopes: { indent: number; text: string }[] = [];
  let nextFold = 0;
  for (let i = 0; i < lines.length && nextFold < folds.length; i++) {
    const line = lines[i]!;
    const text = line.text.trim();
    if (line.kind === 'delete' || !text) continue;
    const indent = (line.text.match(/^[ \t]*/)?.[0] ?? '').replace(/\t/g, '    ').length;
    while (scopes.length && scopes[scopes.length - 1]!.indent >= indent) scopes.pop();
    while (folds[nextFold] && folds[nextFold]!.index + folds[nextFold]!.fold <= i) {
      folds[nextFold]!.scope = scopes[scopes.length - 1]?.text;
      nextFold++;
    }
    scopes.push({ indent, text });
  }
  return rows;
}

/** The file a review is on, in the words its bar shows, and the files either side of it. */
export function fileReviewHeading(review: SourceFileReview) {
  const file = review.files[review.index]!;
  const count = review.files.length;
  const viewed = review.viewed.has(file.path);
  return {
    file,
    title: file.path.split('/').pop(),
    subtitle: review.index + 1 + ' of ' + count + (review.branch ? ' · ' + review.viewed.size + ' viewed' : ' in this commit'),
    viewed,
    advanceLabel: review.branch && !viewed ? 'Viewed · next file' : 'Next file',
    marksViewed: review.branch && !viewed,
    previous: () => review.select((review.index - 1 + count) % count),
    next: () => review.select((review.index + 1) % count),
  };
}
