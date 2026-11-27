import { describe, expect, it } from 'vitest';

import { curriculum } from './curriculum';
import { audioCues, sectionTracks } from './audio.generated';
import { checkAnswer, nextTask } from '../../domain/lessons/course';

describe('Tamil curriculum', () => {
  it('Given the complete deck, When loaded, Then every note is the scored target of an exercise with Tamil audio', async () => {
    const sections = await curriculum.listSections();
    const tasks = sections.flatMap((section) => section.topics.flatMap((topic) => topic.tasks));
    const targets = tasks.flatMap((task) => task.exercises);

    expect(sections.flatMap((section) => section.topics)).toHaveLength(31);
    expect(targets).toHaveLength(1014);
    expect(new Set(targets.map((exercise) => exercise.sourceGuid)).size).toBe(1014);
    expect(Object.keys(audioCues).sort()).toEqual(
      targets.map(({ id }) => id).sort()
    );
    expect(Object.keys(sectionTracks).sort()).toEqual(sections.map(section => section.id).sort());
    for (const section of sections) {
      let previousEnd = -500;
      for (const task of section.topics.flatMap(topic => topic.tasks)) {
        for (const exercise of task.exercises) {
          const cue = audioCues[exercise.id];
          expect(cue?.[0]).toBe(section.id);
          expect(cue?.[1]).toBe(previousEnd + 500);
          expect(cue?.[2]).toBeGreaterThan(cue![1]);
          previousEnd = cue![2];
        }
      }
    }
    for (const exercise of targets) {
      const answer = exercise.kind === 'fill' ? [exercise.words[exercise.blank]] : exercise.words;
      expect(checkAnswer(exercise, answer).correct).toBe(true);
      expect(checkAnswer(exercise, []).correct).toBe(false);
    }
  });

  it('Given an unfinished task, When Practice opens, Then it resumes ahead of other tasks', async () => {
    const [section] = await curriculum.listSections();
    const [first, second] = section.topics[0].tasks;
    expect(nextTask([first, second], {}, second.id)?.id).toBe(second.id);
    expect(nextTask([first, second], { [second.id]: { answers: {}, completed: true } }, second.id)?.id).toBe(first.id);
  });

  it('Given a Tamil answer, When checked, Then Unicode equivalents pass and distinct vowels stay distinct', () => {
    const exercise = { kind: 'translate', tamil: 'கொடி', words: ['கொடி'] } as const;
    expect(checkAnswer(exercise, '  கொடி  '.normalize('NFD')).correct).toBe(true);
    expect(checkAnswer(exercise, 'கோடி').correct).toBe(false);
  });
});
