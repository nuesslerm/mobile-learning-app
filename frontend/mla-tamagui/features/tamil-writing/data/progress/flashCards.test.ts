import { Rating } from 'ts-fsrs';
import { describe, expect, it } from 'vitest';

import { selectCards } from '../../domain/lessons/flashCards';
import { createMemoryTamilStorage, tamilTestCurriculum } from '../../testing';
import { createFlashCardProgress } from './flashCards';

const start = new Date('2026-01-10T12:00:00Z');
const direction = 'english-first';
describe('Tamil flash-card schedules', () => {
  it.each([Rating.Again, Rating.Hard, Rating.Good, Rating.Easy] as const)(
    'Given a new card, When rating %s is tapped twice, Then one scheduled review survives reopening', async (grade) => {
      const storage = createMemoryTamilStorage();
      const progress = createFlashCardProgress(storage);
      await Promise.all([1, 2].map(() => progress.rate('phrase', direction, 0, grade, start)));
      await progress.setDirection('tamil-first');
      const reopened = createFlashCardProgress(storage);
      expect(await reopened.readDirection()).toBe('tamil-first');
      expect(await reopened.read(['phrase'], 'tamil-first')).toEqual({});
      const { phrase } = await reopened.read(['phrase'], direction);
      expect(phrase.history.map((review) => review.rating)).toEqual([grade]);
      expect(new Date(phrase.card.due).getTime()).toBeGreaterThan(start.getTime());
    });

  it('Given two missed phrases and a new one, When their intervals elapse, Then the earliest due phrase comes first', async () => {
    const task = (await tamilTestCurriculum.listSections())[0].topics[0].tasks[0];
    const [first, second, third] = task.exercises;
    const progress = createFlashCardProgress(createMemoryTamilStorage());
    await progress.rate(second.id, direction, 0, Rating.Again, start);
    await progress.rate(first.id, direction, 0, Rating.Again, new Date(+start + 1000));
    const reviews = await progress.read([first.id, second.id, third.id], direction);
    const due = +new Date(reviews[second.id].card.due);
    expect(selectCards(task.exercises, reviews, due - 1).current).toBe(third);
    expect(selectCards(task.exercises, reviews, due).current).toBe(second);
    expect(selectCards(task.exercises, reviews, due + 1000)).toMatchObject({ due: 2, current: second });
  });
});
