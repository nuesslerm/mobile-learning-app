import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';

import { ProblemCatalog } from '../../../../domain/problemCatalog';

export type Reference = { problem: string; code: string };

const HERE = dirname(new URL(import.meta.url).pathname);
export const REFERENCE_DIR = join(HERE, '../fixtures/references/javascript');

const defines = (code: string, name: string): boolean => new RegExp(`^(?:class|function|var|let|const)\\s+${name}\\b`, 'm').test(code);

/** What the harness can call in a problem's code: the class of a harness problem, else `Solution`, or the function named for the method. */
export function entryPointsOf(problemId: string): string[] {
  const problem = ProblemCatalog.getByIdOrThrow(problemId);
  return [problem.harness?.className ?? 'Solution', ...(problem.methodName === null ? [] : [problem.methodName])];
}

/** One known-good solution per file, `<problem id>.js`; a file that names no catalog problem or defines none of the problem's entry points fails the load. */
export function loadReferenceCorpus(dir: string = REFERENCE_DIR): Reference[] {
  return readdirSync(dir, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith('.js'))
    .map((entry) => entry.name)
    .sort()
    .map((file) => {
      const problem = file.slice(0, -'.js'.length);
      if (!ProblemCatalog.getById(problem)) throw new Error(`${file} is named for no catalog problem`);
      const code = readFileSync(join(dir, file), 'utf8');
      const names = entryPointsOf(problem);
      if (!names.some((name) => defines(code, name))) throw new Error(`${file} defines none of ${names.join(', ')}`);
      return { problem, code };
    });
}

export function referenceFor(problemId: string, corpus: Reference[] = loadReferenceCorpus()): string {
  const found = corpus.find((entry) => entry.problem === problemId);
  if (!found) throw new Error(`no reference solution for ${problemId}`);
  return found.code;
}
