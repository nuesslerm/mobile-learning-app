export type ColdStartMark = 'mount' | 'loadStart' | 'ready' | 'hydrated';

export type ColdStartReport = {
  nth: number;
  htmlBytes: number;
  mountToReadyMs: number;
  readyToHydratedMs: number;
  totalMs: number;
  evalMs?: number;
  loadStartToReadyMs?: number;
};

export type ColdStart = {
  readonly nth: number;
  mark(name: ColdStartMark): void;
  /** The report of this editor's first hydration; null while a mark is missing and once it has been taken. */
  take(page: { evalMs?: number }): ColdStartReport | null;
};

let opened = 0;

/** The editor's index in this JS runtime, counted from 1: the first open is the coldest. */
export function nextEditorNth(): number {
  opened += 1;
  return opened;
}

export function createColdStart(options: { clock: () => number; nth: number; htmlBytes: number }): ColdStart {
  const marks = new Map<ColdStartMark, number>();
  let taken = false;
  return {
    nth: options.nth,
    mark(name) {
      if (!marks.has(name)) marks.set(name, options.clock());
    },
    take(page) {
      const mount = marks.get('mount');
      const ready = marks.get('ready');
      const hydrated = marks.get('hydrated');
      if (taken || mount === undefined || ready === undefined || hydrated === undefined) return null;
      taken = true;
      const loadStart = marks.get('loadStart');
      return {
        nth: options.nth,
        htmlBytes: options.htmlBytes,
        mountToReadyMs: ready - mount,
        readyToHydratedMs: hydrated - ready,
        totalMs: hydrated - mount,
        ...(page.evalMs === undefined ? {} : { evalMs: page.evalMs }),
        ...(loadStart === undefined ? {} : { loadStartToReadyMs: ready - loadStart }),
      };
    },
  };
}
