import { Logger } from '@lib/logging';
import React from 'react';
import { Keyboard } from 'react-native';

import { consoleResultOf, consoleResultOfFailure, type ConsoleResult } from '../../domain/console/consoleResult';
import { planRun, type RunMode, type RunPlan } from '../../domain/console/runPlan';
import { consoleCases, EMPTY_DRAFTS, MAX_CUSTOM_CASES, selectedCase, type ConsoleCase } from '../../domain/console/testcases';
import type { Blind75Problem } from '../../domain/models';
import type { CodeRunnerPort } from '../../domain/ports';
import type { RunEditor } from '../../domain/run/runEditor';
import { runLocalOnce } from '../run/runLocalOnce';
import type { ErrorJump } from '../run/useErrorJump';
import { useConsoleCasesStore } from './consoleCasesStore';

export type ConsoleTab = 'testcase' | 'result';
export type ConsoleCloseReason = 'sheet' | 'editor-focus' | 'plan-tab' | 'voice' | 'back' | 'line';

/** What the screen lends a run: the runner, the editor, the buffer as it is now and what to do around the answer. */
export type ConsoleRuntime = {
  runner: CodeRunnerPort;
  editor: () => RunEditor | null | undefined;
  latest: () => { text: string; lang: string };
  dismissKeyboard: () => void;
  errorJump: ErrorJump;
  /** The error line was marked again and the page should show the editor. */
  onRevealed: (line: number) => void;
  /** The answer was dropped because the code moved while it ran. */
  onStale: () => void;
};

export type ConsoleOutcome = { plan: RunPlan; result: ConsoleResult; marked: boolean };

export type ConsoleState = {
  open: boolean;
  tab: ConsoleTab;
  cases: ConsoleCase[];
  selected: ConsoleCase | null;
  atCap: boolean;
  hasBundled: boolean;
  /** The run in flight, Run or Submit. */
  pending: RunMode | null;
  outcome: ConsoleOutcome | null;
  fieldError: { name: string; message: string } | null;
  openConsole: (tab?: ConsoleTab) => void;
  closeConsole: (reason: ConsoleCloseReason) => void;
  setTab: (tab: ConsoleTab) => void;
  select: (id: string) => void;
  addCase: () => string | null;
  editField: (id: string, name: string, text: string) => void;
  removeCase: (id: string) => void;
  resetCases: () => void;
  run: (mode: RunMode) => Promise<void>;
  /** The result goes away when the code it describes does; `text` is the buffer now. */
  onCodeChanged: (text: string) => void;
  clearResult: () => void;
  showLine: () => Promise<void>;
  /** The chip a result names by its label, when it is still there. */
  caseIdOf: (label: string) => string | null;
  openCase: (label: string) => void;
};

/** The console's open state and tab, the testcases of the problem on screen (session-only) and the runs made from it. */
export function useConsole(problem: Blind75Problem | undefined, runtime: ConsoleRuntime): ConsoleState {
  const problemId = problem?.id;
  const [open, setOpen] = React.useState(false);
  const openRef = React.useRef(false);
  const [tab, setTabState] = React.useState<ConsoleTab>('testcase');
  const [pending, setPending] = React.useState<RunMode | null>(null);
  const pendingRef = React.useRef(false);
  const [outcome, setOutcome] = React.useState<ConsoleOutcome | null>(null);
  const [fieldError, setFieldError] = React.useState<{ name: string; message: string } | null>(null);
  const sourceRef = React.useRef<string | null>(null);
  const mountedRef = React.useRef(true);
  const runtimeRef = React.useRef(runtime);
  runtimeRef.current = runtime;
  React.useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const drafts = useConsoleCasesStore((state) => (problemId ? state.drafts[problemId] : undefined)) ?? EMPTY_DRAFTS;
  const selectedId = useConsoleCasesStore((state) => (problemId ? (state.selected[problemId] ?? null) : null));
  const store = useConsoleCasesStore;

  const cases = React.useMemo(() => (problem ? consoleCases(problem, drafts) : []), [problem, drafts]);
  const selected = selectedCase(cases, selectedId);
  const hasBundled = (problem?.testCases ?? []).length > 0;

  const openConsole = React.useCallback(
    (next: ConsoleTab = 'testcase') => {
      setTabState(next);
      if (!openRef.current) Logger.info('editor_v2.console.open', { problemId, tab: next });
      openRef.current = true;
      setOpen(true);
    },
    [problemId]
  );
  const closeConsole = React.useCallback(
    (reason: ConsoleCloseReason) => {
      if (openRef.current) Logger.info('editor_v2.console.close', { problemId, reason });
      openRef.current = false;
      setOpen(false);
    },
    [problemId]
  );
  const setTab = React.useCallback((next: ConsoleTab) => setTabState(next), []);
  const select = React.useCallback((id: string) => problemId && store.getState().select(problemId, id), [problemId, store]);
  const addCase = React.useCallback(() => (problem ? store.getState().addCase(problem) : null), [problem, store]);
  const editField = React.useCallback(
    (id: string, name: string, text: string) => {
      setFieldError(null);
      if (problemId) store.getState().editField(problemId, id, name, text);
    },
    [problemId, store]
  );
  const removeCase = React.useCallback((id: string) => {
    if (!problem) return;
    if (selected?.id === id) setFieldError(null);
    store.getState().removeCase(problem, id);
  }, [problem, selected?.id, store]);

  const clearResult = React.useCallback(() => {
    sourceRef.current = null;
    setOutcome(null);
    runtimeRef.current.errorJump.clear();
  }, []);
  const resetCases = React.useCallback(() => {
    if (!problem) return;
    Logger.info('editor_v2.console.reset_cases', { problemId });
    setFieldError(null);
    clearResult();
    store.getState().reset(problem);
  }, [problem, problemId, store, clearResult]);

  const onCodeChanged = React.useCallback(
    (text: string) => {
      if (sourceRef.current !== null && text !== sourceRef.current) clearResult();
    },
    [clearResult]
  );

  const run = React.useCallback(
    async (mode: RunMode) => {
      if (!problem || pendingRef.current) return;
      const rt = runtimeRef.current;
      const latest = rt.latest();
      const planned = planRun({ problem, lang: latest.lang, code: latest.text, mode, drafts: store.getState().drafts[problem.id] ?? EMPTY_DRAFTS, selectedId: store.getState().selected[problem.id] ?? null });
      if (!planned.ok) {
        Logger.info('editor_v2.console.run_refused', { problemId, mode, reason: planned.reason });
        if (planned.reason === 'bad-field') {
          setFieldError({ name: planned.name, message: planned.message });
          openConsole('testcase');
        }
        return;
      }
      const { plan } = planned;
      setFieldError(null);
      rt.dismissKeyboard();
      Keyboard.dismiss();
      clearResult();
      pendingRef.current = true;
      setPending(mode);
      Logger.info('editor_v2.console.run_start', { problemId, mode, cases: plan.cases.length, lang: plan.lang });
      const done = await runLocalOnce({
        runner: rt.runner,
        editor: rt.editor(),
        given: plan.params,
        isCurrent: (params) => {
          const now = runtimeRef.current.latest();
          return now.text === params.code && now.lang === params.lang;
        },
        problemId,
        mode: `console-${mode}`,
      });
      pendingRef.current = false;
      if (!mountedRef.current) return;
      setPending(null);
      if (done.kind === 'discarded') {
        Logger.info('editor_v2.console.run_end', { problemId, mode, verdict: 'discarded', ms: done.ms });
        runtimeRef.current.onStale();
        return;
      }
      const result = done.kind === 'failed' ? consoleResultOfFailure(done.error) : consoleResultOf(plan, done.response);
      sourceRef.current = done.kind === 'answered' ? done.params.code : latest.text;
      setOutcome({ plan, result, marked: false });
      Logger.info('editor_v2.console.run_end', { problemId, mode, verdict: result.kind, ms: done.ms });
      openConsole('result');
      if (done.kind !== 'answered' || !done.checkpoint) return;
      const place = 'errorAt' in result ? result.errorAt : undefined;
      if (!place) return;
      const jumped = await runtimeRef.current.errorJump.show(done.checkpoint, place);
      if (jumped.kind === 'shown' && mountedRef.current) setOutcome((was) => (was && was.result === result ? { ...was, marked: true } : was));
    },
    [problem, problemId, store, openConsole, clearResult]
  );

  const showLine = React.useCallback(async () => {
    const jumped = await runtimeRef.current.errorJump.showAgain();
    if (jumped.kind !== 'shown') return;
    closeConsole('line');
    runtimeRef.current.onRevealed(jumped.line);
  }, [closeConsole]);

  const caseIdOf = React.useCallback(
    (label: string) => {
      const id = outcome?.plan.cases.find((entry) => entry.label === label)?.id ?? null;
      return id && cases.some((entry) => entry.id === id) ? id : null;
    },
    [outcome, cases]
  );
  const openCase = React.useCallback(
    (label: string) => {
      const id = caseIdOf(label);
      if (!id) return;
      select(id);
      setTabState('testcase');
    },
    [caseIdOf, select]
  );

  return {
    open,
    tab,
    cases,
    selected,
    atCap: drafts.items.length >= MAX_CUSTOM_CASES,
    hasBundled,
    pending,
    outcome,
    fieldError,
    openConsole,
    closeConsole,
    setTab,
    select,
    addCase,
    editField,
    removeCase,
    resetCases,
    run,
    onCodeChanged,
    clearResult,
    showLine,
    caseIdOf,
    openCase,
  };
}
