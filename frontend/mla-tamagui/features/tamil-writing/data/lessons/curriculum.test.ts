import { describe, expect, it } from 'vitest';

import { curriculum } from './curriculum';
import { tamilAudio } from './audio.generated';
import packedAudio from './audio.packed.json';
import { checkAnswer, nextTask } from '../../domain/lessons/course';

describe('Tamil curriculum', () => {
  it('Given the complete deck, When loaded, Then every note is the scored target of an exercise with Tamil audio', async () => {
    const sections = await curriculum.listSections();
    const tasks = sections.flatMap((section) => section.topics.flatMap((topic) => topic.tasks));
    const targets = tasks.flatMap((task) => task.exercises);

    expect(sections.flatMap((section) => section.topics)).toHaveLength(31);
    expect(targets).toHaveLength(1014);
    expect(new Set(targets.map((exercise) => exercise.sourceGuid)).size).toBe(1014);
    expect([...Object.keys(tamilAudio), ...Object.keys(packedAudio)].sort()).toEqual(
      targets.map(({ id }) => id).sort()
    );
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
