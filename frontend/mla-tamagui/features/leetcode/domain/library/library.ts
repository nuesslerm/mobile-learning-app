import type { Blind75Problem, Difficulty } from '../models';

export type LibraryRow = {
  id: string;
  number: number;
  title: string;
  difficulty: Difficulty;
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

}

export interface LibraryCoverPort {
  pick(): Promise<string | null>;
}
