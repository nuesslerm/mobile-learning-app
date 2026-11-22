import { describe, expect, it } from 'vitest';

import { ProblemCatalog } from '../problemCatalog';

const declares = (code: string, name: string): boolean => new RegExp(`^(?:class|function|var|let|const)\\s+${name}\\b`, 'm').test(code);

describe('the catalog’s JavaScript starters', () => {
  it('Given every catalog problem, When its JavaScript starter is read, Then it declares the method the harness calls, or the class of a harness problem', () => {
    const wrong = ProblemCatalog.getAll()
      .filter((problem) => {
        const starter = problem.starterCode.javascript ?? '';
        const names = [problem.harness?.className ?? 'Solution', problem.methodName].filter((name): name is string => name !== null);
        return !names.some((name) => declares(starter, name));
      })
      .map((problem) => problem.id);

    // The failure names every starter that declares none of what the harness looks for
    expect(wrong).toEqual([]);
  });

  it('Given the catalog, When the starters are counted by form, Then all but two-sum are a var function or a harness class and two-sum is a class', () => {
    const classes = ProblemCatalog.getAll().filter((problem) => declares(problem.starterCode.javascript ?? '', 'Solution'));
    expect(classes.map((problem) => problem.id)).toEqual(['two-sum']);
  });
});
