import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';

import { ProblemCatalog } from '../../../../domain/problemCatalog';

export type Reference = { problem: string; code: string };

const HERE = dirname(new URL(import.meta.url).pathname);
export const REFERENCE_DIR = join(HERE, '../fixtures/references/python3');

/** The class a reference must define: the one the catalog's python3 starter defines. */
export function classNameOf(problemId: string): string {
  const starter = ProblemCatalog.getByIdOrThrow(problemId).starterCode.python3 ?? '';
  const name = /^class (\w+)/m.exec(starter)?.[1];
  if (!name) throw new Error(`the python3 starter of ${problemId} defines no class`);
  return name;
}

/** One known-good solution per file, `<problem id>.py`; a file that names no catalog problem or lacks the problem's class fails the load. */
export function loadReferenceCorpus(dir: string = REFERENCE_DIR): Reference[] {
  return readdirSync(dir, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith('.py'))
    .map((entry) => entry.name)
    .sort()
    .map((file) => {
      const problem = file.slice(0, -'.py'.length);
      if (!ProblemCatalog.getById(problem)) throw new Error(`${file} is named for no catalog problem`);
      const code = readFileSync(join(dir, file), 'utf8');
      const className = classNameOf(problem);
      if (!new RegExp(`^class ${className}\\b`, 'm').test(code)) throw new Error(`${file} does not define class ${className}`);
      return { problem, code };
    });
}

export function referenceFor(problemId: string, corpus: Reference[] = loadReferenceCorpus()): string {
  const found = corpus.find((entry) => entry.problem === problemId);
  if (!found) throw new Error(`no reference solution for ${problemId}`);
  return found.code;
}
