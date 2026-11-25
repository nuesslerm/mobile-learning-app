import { describe, expect, it } from 'vitest';

import { collectionRows } from '../../domain/library/collections';
import { bundledLibrary } from './libraryContent';

describe('the bundled library', () => {
  it('Given the NeetCode deck, When each topic is read, Then 150 distinct lessons carry the statement, video and worked code', () => {
    const rows = collectionRows(bundledLibrary, 'NeetCode 150');
    expect(new Set(rows.map(({ id }) => id)).size).toBe(150);
    expect(new Set(bundledLibrary.studyPlan.map(({ group }) => group)).size).toBe(18);
    for (const { id } of rows) {
      const lesson = bundledLibrary.lesson(id);
      expect(lesson?.video, id).toMatch(/^https:\/\/www.youtube.com\/watch/);
      expect(lesson?.blocks.some((block) => block.kind === 'pre'), id).toBe(true);
    }
  });

  it('Given a visible problem row, When it opens, Then its statement and an editor starter exist', () => {
    for (const { id, number } of bundledLibrary.rows) {
      const problem = bundledLibrary.problem(id);
      expect(problem?.description.trim(), id).toBeTruthy();
      expect(problem?.leetcodeId, id).toBe(number);
      expect(problem?.starterCode.javascript || problem?.starterCode.python3, id).toBeTruthy();
    }
  });
});
