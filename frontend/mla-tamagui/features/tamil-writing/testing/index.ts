import type { WritingCheckPort, WritingVerdict } from '../domain/ports';

/** A marker that answers each call with the next verdict, or an error to throw. */
export function createFakeWritingCheck(answers: (WritingVerdict | Error)[]): WritingCheckPort & { calls: number } {
  const queue = [...answers];
  const fake = {
    calls: 0,
    async check() {
      fake.calls += 1;
      const next = queue.shift();
      if (!next) throw new Error('fake writing check has no answer left');
      if (next instanceof Error) throw next;
      return next;
    },
  };
  return fake;
}

export { createHttpWritingCheck } from '../data/httpWritingCheck';
export { WritingCheckError } from '../domain/ports';
export type { TamilWritingPorts, WritingVerdict } from '../domain/ports';
