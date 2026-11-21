import { familyOf } from '../../syntax';
import { parentKey } from '../destination';
import { carryPos, frozenLines, linesOf, shiftPrefix } from '../indent';
import type { Planner, StepOf } from '../plan';
import { statementShapes } from '../verify';

/**
 * Puts the whole lines of a run under a new header, one indent unit deeper: `header:` in Python, `header {` and a closing `}` line in braces. The header is the line without its colon or
 * brace. What the result must be is one compound whose block holds exactly the run, in the place the run was: a header that does not make one (`if (x) foo();`, `else`) is refused.
 */
export const planWrap: Planner<StepOf<'wrapNode'>> = (step, context) => {
  const source = context.locate(step.node, step.expect);
  if (!source.ok) return source;
  const { run, extent, indentPrefix, unterminated, parent } = source.value;
  const { text, root, language } = context.input;
  const header = step.header.trim();
  // the grammars take a `try` with no clause, and the code would not run: one line cannot carry the clause it needs
  if (/^try\b/.test(header)) return { ok: false, error: { code: 'invalid', message: 'a try needs an except or catch clause, which a one-line header cannot give', path: `steps[${context.step}].header` } };

  const inner = indentPrefix + context.indentUnit;
  const lines = linesOf(text, extent);
  const shifted = shiftPrefix(lines, indentPrefix, inner, frozenLines(text, root, language));
  const braces = familyOf(language) === 'braces';
  const opener = `${indentPrefix}${header}${braces ? ' {' : ':'}`;
  const insert = [opener, ...shifted, ...(braces ? [`${indentPrefix}}`] : [])].join('\n') + (unterminated ? '' : '\n');
  const carry = carryPos(lines, shifted);
  return {
    ok: true,
    value: {
      edits: [{ from: extent.from, to: extent.to, insert }],
      landed: { edit: 0, from: 0, to: insert.length },
      expectation: {
        removed: [extent],
        writes: { parent: parentKey(language, parent), shapes: [{ is: 'compound', children: [{ is: 'block', children: statementShapes(language, run) }] }] },
      },
      follow: { range: { from: extent.from, to: unterminated ? extent.to : extent.to - 1 }, offset: (pos) => opener.length + 1 + carry(pos) },
    },
  };
};
