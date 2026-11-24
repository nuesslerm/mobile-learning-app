import type { Blind75Problem, Difficulty } from '../models';

export type LibraryRow = {
  id: string;
  number: number;
  title: string;
  difficulty: Difficulty;
  acceptance?: string;
  frequency?: string;
  solved?: boolean;
  searchTerms?: string;
};

export type SavedProblemList = {
  id: string;
  name: string;
  description: string;
  private: boolean;
  problemIds: string[];
  coverUri?: string;
};

export type LibrarySnapshot = {
  lists: SavedProblemList[];
  lastOpened: string | null;
  savedCollections?: string[];
};

export interface LibraryStoragePort {
  read(): Promise<LibrarySnapshot | null>;
  write(snapshot: LibrarySnapshot): Promise<void>;
}

export function filterLibrary(rows: readonly LibraryRow[], query: string): LibraryRow[] {
  const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  return rows.filter((row) => {
    const text = `${row.number} ${row.title} ${row.searchTerms ?? ''}`.toLowerCase();
    return terms.every((term) => text.includes(term));
  });
}

export function putInList(list: SavedProblemList, problemId: string): SavedProblemList {
  return list.problemIds.includes(problemId) ? list : { ...list, problemIds: [...list.problemIds, problemId] };
}

export type Submission = {
  id: string;
  status: 'Accepted' | 'Runtime Error';
  language: string;
  date: string;
  runtime?: string;
  memory?: string;
};

export type Solution = {
  id: string;
  author: string;
  title: string;
  summary: string;
  language: string;
  metrics: string;
};

export interface LibraryLesson {
  url: string;
  video: string | null;
  prerequisites: { title: string; url: string }[];
  blocks: { kind: 'h2' | 'h3' | 'p' | 'li' | 'pre'; text: string }[];
}

export interface StudyEntry {
  id: string;
  group: string;
  topicId: string;
  topics: string[];
}

export interface LibraryContentPort {
  studyPlan: readonly StudyEntry[];
  lesson(id: string): LibraryLesson | undefined;
  rows: readonly LibraryRow[];
  problem(id: string): Blind75Problem | undefined;
  submissions(id: string): ReadonlyArray<Submission>;
  submission(id: string, submissionId: string): Promise<Submission | undefined>;
  submissionCode: string;
  solutions(id: string): ReadonlyArray<Solution>;
  companies: ReadonlyArray<readonly [string, string, string]>;
  hints(problem: Blind75Problem): readonly string[];
}

export interface LibraryCoverPort {
  pick(): Promise<string | null>;
}
