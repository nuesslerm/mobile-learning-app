import { create } from 'zustand';

import { addDraft, EMPTY_DRAFTS, editDraft, removeDraft, resetDrafts, type ConsoleDrafts } from '../../domain/console/testcases';
import type { Blind75Problem } from '../../domain/models';

type ConsoleCasesState = {
  drafts: Record<string, ConsoleDrafts>;
  selected: Record<string, string | null>;
  select: (problemId: string, id: string) => void;
  /** A custom case seeded with the selected one, selected; its id, or null at the cap. */
  addCase: (problem: Blind75Problem) => string | null;
  editField: (problemId: string, id: string, name: string, text: string) => void;
  removeCase: (problem: Blind75Problem, id: string) => void;
  reset: (problem: Blind75Problem) => void;
};

/** The custom testcases and the chosen chip of each problem, for this session only: nothing is persisted. */
export const useConsoleCasesStore = create<ConsoleCasesState>()((set, get) => ({
  drafts: {},
  selected: {},
  select: (problemId, id) => set((state) => ({ selected: { ...state.selected, [problemId]: id } })),
  addCase: (problem) => {
    const { drafts, selected } = get();
    const added = addDraft(problem, drafts[problem.id] ?? EMPTY_DRAFTS, selected[problem.id] ?? null);
    if (!added) return null;
    set({ drafts: { ...drafts, [problem.id]: added.drafts }, selected: { ...selected, [problem.id]: added.id } });
    return added.id;
  },
  editField: (problemId, id, name, text) =>
    set((state) => ({ drafts: { ...state.drafts, [problemId]: editDraft(state.drafts[problemId] ?? EMPTY_DRAFTS, id, name, text) } })),
  removeCase: (problem, id) =>
    set((state) => {
      const removed = removeDraft(problem, state.drafts[problem.id] ?? EMPTY_DRAFTS, id, state.selected[problem.id] ?? null);
      return { drafts: { ...state.drafts, [problem.id]: removed.drafts }, selected: { ...state.selected, [problem.id]: removed.selectedId } };
    }),
  reset: (problem) =>
    set((state) => {
      const fresh = resetDrafts(problem);
      return { drafts: { ...state.drafts, [problem.id]: fresh.drafts }, selected: { ...state.selected, [problem.id]: fresh.selectedId } };
    }),
}));
