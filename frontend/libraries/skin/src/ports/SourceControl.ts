export interface SourceCommit {
  sha: string;
  shortSha: string;
  subject: string;
  author: string;
  authoredAtMs: number;
  parent: string | null;
}

export interface SourceControlProps {
  backLabel?: string;
  tab: 'changes' | 'pr';
  selectTab: (tab: 'changes' | 'pr') => void;
  prPanel: {
    result?: { branch: string | null; pullRequest: {
      number: number; title: string; state: 'open' | 'merged' | 'closed'; draft: boolean;
      base: string; head: string; author: string; url: string;
      description: string; mergeable: string | null; reviewDecision: string | null;
      files: number | null; additions: number | null; deletions: number | null;
      checks: { passing: number; failing: number; pending: number } | null;
    } | null };
    error?: string;
    refresh: () => void;
    open: (url: string) => void;
    parseReference: (text: string) => number | null;
    link: (number: number) => Promise<void>;
  };
  page: 'changes' | 'commits' | 'detail';
  comparison?: {
    base: string;
    ahead: number;
    files: SourceFile[];
    limited: boolean;
  } | null;
  selected?: SourceCommit;
  detail?: {
    body: string;
    initial: boolean;
    limited: boolean;
    files: SourceFile[];
  };
  openFile: (index: number) => void;
  review?: SourceFileReview;
  openBranches: () => void;
  switchingBranch: boolean;
  picker?: {
    branches?: readonly string[];
    current?: string | null;
    truncated?: boolean;
    error?: string;
    retry: () => void;
    close: () => void;
    select: (branch: string) => void;
  };
  openHistory: () => void;
  openCommit: (commit: SourceCommit) => void;
  branch: string | null;
  head: string | null;
  commits: SourceCommit[];
  loading: boolean;
  loadingMore: boolean;
  error?: string;
  onBack: () => void;
  onRefresh: () => void;
  onLoadMore?: () => void;
}

export interface SourceFile {
  path: string;
  status: string;
  additions?: number;
  deletions?: number;
  binary: boolean;
}

export interface SourceCodeLine {
  kind: 'context' | 'add' | 'delete';
  text: string;
  newLineNumber?: number;
  oldLineNumber?: number;
}
export interface SourceFileReview {
  index: number;
  files: SourceFile[];
  branch: boolean;
  viewed: ReadonlySet<string>;
  lines?: SourceCodeLine[];
  message?: string;
  loading: boolean;
  retry?: () => void;
  back: () => void;
  select: (index: number) => void;
  toggleViewed: () => void;
  advance: () => void;
}
