export interface SourceCommit {
  sha: string;
  shortSha: string;
  subject: string;
  author: string;
  authoredAtMs: number;
  parent: string | null;
}

export interface SourceControlProps {
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
