import { createEmptyCard, fsrs, Rating, type CardInput, type Grade, type ReviewLogInput } from 'ts-fsrs';

import type { Exercise } from './course';

export type Direction = 'english-first' | 'tamil-first';
export const ratings = [Rating.Again, Rating.Hard, Rating.Good, Rating.Easy] as const;
export const ratingLabels = { [Rating.Again]: 'Again', [Rating.Hard]: 'Hard', [Rating.Good]: 'Good', [Rating.Easy]: 'Easy' };
export type CardReview = { card: CardInput; history: readonly ReviewLogInput[] };
export type Reviews = Record<string, CardReview>;
export type FlashCardProgressPort = {
  readDirection(): Promise<Direction>;
  setDirection(direction: Direction): Promise<void>;
  read(ids: readonly string[], direction: Direction): Promise<Reviews>;
  rate(id: string, direction: Direction, reps: number, grade: Grade, now: Date): Promise<CardReview>;
};

const scheduler = fsrs({ request_retention: 0.9, enable_fuzz: false });

export function preview(review: CardReview | undefined, now: Date) {
  return scheduler.repeat(review?.card ?? createEmptyCard(now), now);
}

export function selectCards(exercises: readonly Exercise[], reviews: Reviews, now: number) {
  const dueTime = (exercise: Exercise) => new Date(reviews[exercise.id].card.due).getTime();
  const seen = exercises.filter((exercise) => reviews[exercise.id]).sort((a, b) => dueTime(a) - dueTime(b));
  const due = seen.filter((exercise) => dueTime(exercise) <= now);
  const fresh = exercises.filter((exercise) => !reviews[exercise.id]);
  const future = seen[due.length];
  return { current: due[0] ?? fresh[0], due: due.length, fresh: fresh.length, nextDue: future && dueTime(future) };
}

export function intervalLabel(milliseconds: number) {
  const minutes = Math.max(1, Math.round(milliseconds / 60_000));
  if (minutes < 60) return minutes + ' min';
  if (minutes < 1440) return Math.round(minutes / 60) + ' hr';
  return Math.round(minutes / 1440) + ' d';
}
