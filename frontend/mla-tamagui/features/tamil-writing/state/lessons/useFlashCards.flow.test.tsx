import { act, renderHook, waitFor } from '@lib/flow-test-harness';
import type { ReactNode } from 'react';
import { Rating } from 'ts-fsrs';
import { afterEach, expect, it, vi } from 'vitest';

import { createCourseProgress, createFlashCardProgress, createMemoryTamilStorage, tamilTestCurriculum } from '../../testing';
import { TamilWritingPortsProvider } from '../ports';
import { useFlashCards } from './useFlashCards';

afterEach(() => vi.useRealTimers());

it('Given a revealed card, when a double tap meets a failed save, then one retry records one review and advances', async () => {
  const now = new Date('2026-01-10T12:00:00Z');
  vi.useFakeTimers({ toFake: ['Date'] });
  vi.setSystemTime(now);
  const [section] = await tamilTestCurriculum.listSections();
  const [task] = section.topics[0].tasks;
  const [first, second] = task.exercises;
  const storage = createMemoryTamilStorage();
  let failWrite: (cause: Error) => void = () => { throw new Error('The write has not started'); };
  const failure = new Promise<void>((_, reject) => { failWrite = reject; });
  const setItem = vi.fn(storage.setItem).mockImplementationOnce(() => failure);
  const flashCards = createFlashCardProgress({ ...storage, setItem });
  const ports = {
    flashCards, curriculum: tamilTestCurriculum, courseProgress: createCourseProgress(storage),
    audio: { play: async () => {}, stop() {} },
  };
  const { result } = renderHook(() => useFlashCards(task), {
    wrapper: ({ children }: { children: ReactNode }) => <TamilWritingPortsProvider value={ports}>{children}</TamilWritingPortsProvider>,
  });
  await waitFor(() => expect(result.current.current?.id).toBe(first.id));
  act(() => result.current.reveal());
  let rating: Promise<void>;
  act(() => {
    rating = result.current.rate(Rating.Good);
    void result.current.rate(Rating.Good);
  });
  await waitFor(() => expect(setItem).toHaveBeenCalledTimes(1));
  await act(async () => { failWrite(new Error('Disk full')); await rating; });
  expect(result.current.revealed).toBe(true);
  expect(result.current.current?.id).toBe(first.id);
  expect(await flashCards.read([first.id], 'english-first')).toEqual({});

  await act(async () => { await Promise.all([result.current.rate(Rating.Good), result.current.rate(Rating.Good)]); });
  expect(result.current.current?.id).toBe(second.id);
  expect(result.current.revealed).toBe(false);
  const saved = (await flashCards.read([first.id], 'english-first'))[first.id];
  expect(saved.history.map((review) => [review.rating, new Date(review.review).toISOString()])).toEqual([[Rating.Good, now.toISOString()]]);
  expect(await flashCards.read([first.id], 'tamil-first')).toEqual({});
});
