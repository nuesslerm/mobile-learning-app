import { Logger } from '@lib/logging';
import { useCallback, useEffect, useRef, useState } from 'react';

import type { Task } from '../../domain/lessons/course';
import { preview, selectCards, type Direction, type Reviews, type ratings } from '../../domain/lessons/flashCards';
import { useTamilWritingPorts } from '../ports';

type Session = { direction: Direction; reviews: Reviews; now: number };

export function useFlashCards(task: Task) {
  const { flashCards, audio } = useTamilWritingPorts();
  const [playedId, setPlayedId] = useState<string>();
  const [session, setSession] = useState<Session>();
  const [revealed, setRevealed] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string>();
  const busy = useRef(false);
  const protect = useCallback(async (action: () => Promise<void>) => {
    if (busy.current) return;
    busy.current = true;
    setPending(true);
    setError(undefined);
    try {
      await action();
    } catch (cause) {
      Logger.withError(cause).error('tamil.flash_cards.action_failed');
      setError('Couldn’t finish that action. Try again.');
    } finally {
      busy.current = false;
      setPending(false);
    }
  }, []);

  const load = useCallback((selected?: Direction) => protect(async () => {
    const direction = selected ?? await flashCards.readDirection();
    const reviews = await flashCards.read(task.exercises.map((exercise) => exercise.id), direction);
    if (selected) await flashCards.setDirection(direction);
    setRevealed(false);
    setSession({ direction, reviews, now: Date.now() });
  }), [flashCards, protect, task]);
  useEffect(() => { void load(); }, [load]);

  const queue = session && selectCards(task.exercises, session.reviews, session.now);
  const current = queue?.current;
  const nextDue = queue?.nextDue;
  useEffect(() => () => audio.stop(), [audio, current?.id, session?.direction]);
  useEffect(() => {
    if (current || !nextDue) return;
    const timer = setTimeout(() => setSession((previous) => previous && { ...previous, now: Date.now() }),
      Math.min(2_147_483_647, Math.max(1, nextDue - Date.now())));
    return () => clearTimeout(timer);
  }, [current, nextDue, session?.now]);

  const review = current && session?.reviews[current.id];
  async function rate(grade: typeof ratings[number]) {
    if (!current || !session || !revealed) return;
    await protect(async () => {
      const saved = await flashCards.rate(current.id, session.direction, review?.card.reps ?? 0, grade, new Date());
      setSession({ ...session, reviews: { ...session.reviews, [current.id]: saved }, now: Date.now() });
      setRevealed(false);
    });
  }
  return {
    session, queue, current, pending, error, revealed, load, rate,
    reveal: () => setRevealed(true),
    options: revealed && session && preview(review, new Date(session.now)),
    played: Boolean(current && current.id === playedId),
    play: () => protect(async () => { if (current) await audio.play(current.id, false, () => setPlayedId(current.id)); }),
  };
}
