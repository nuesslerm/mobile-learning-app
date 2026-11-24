import { Logger } from '@lib/logging';
import { useEffect, useRef, useState } from 'react';

import { reviewExercises, type Answer, type CourseProgress, type CourseUpdate, type Outcome, type Task, type Verdict } from '../../domain/lessons/course';
import { useTamilWritingPorts } from '../ports';

type Save = (update: CourseUpdate) => Promise<CourseProgress>;

export function useTaskPractice(topicId: string, task: Task, saved: CourseProgress, save: Save) {
  const { curriculum, audio } = useTamilWritingPorts();
  const [answers, setAnswers] = useState(saved.tasks[task.id]?.answers ?? {});
  const [round, setRound] = useState(() => task.exercises.filter((exercise) => !answers[exercise.id]));
  const [initialCount, setInitialCount] = useState(task.exercises.length - round.length);
  const [index, setIndex] = useState(0);
  const [feedback, setFeedback] = useState<Verdict>();
  const [preview, setPreview] = useState(round.length === task.exercises.length);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string>();
  const busy = useRef(false);
  const exercise = round[index];
  const finishedBeforeMount = task.exercises.every((item) => saved.tasks[task.id]?.answers[item.id]);

  useEffect(() => {
    void save({ kind: finishedBeforeMount ? 'complete' : 'open', topicId, taskId: task.id }).catch((cause: unknown) => {
      Logger.withError(cause).error('tamil.task.open_failed');
    });
  }, [finishedBeforeMount, save, task.id, topicId]);

  useEffect(() => () => audio.stop(), [audio, exercise?.id]);

  async function protect(action: () => Promise<void>) {
    if (busy.current) return;
    busy.current = true;
    setPending(true);
    setError(undefined);
    try {
      await action();
    } catch (cause) {
      Logger.withError(cause).error('tamil.task.save_failed');
      setError('Couldn’t save your progress. Try again.');
    } finally {
      busy.current = false;
      setPending(false);
    }
  }

  async function record(answer?: Answer) {
    if (!exercise || feedback) return;
    await protect(async () => {
      const verdict = answer === undefined ? undefined : await curriculum.check(task.id, exercise.id, answer);
      const outcome: Outcome = verdict ? (verdict.correct ? 'correct' : 'missed') : 'skipped';
      await save({ kind: 'answer', topicId, taskId: task.id, exerciseId: exercise.id, outcome });
      setAnswers((previous) => ({ ...previous, [exercise.id]: outcome }));
      if (verdict) setFeedback(verdict);
      else await advance();
    });
  }

  async function advance() {
    if (index + 1 >= round.length) await save({ kind: 'complete', topicId, taskId: task.id });
    setFeedback(undefined);
    setIndex(index + 1);
  }

  function restart(reviewOnly: boolean) {
    setRound(reviewOnly ? reviewExercises(task, { answers, completed: true }) : task.exercises.slice());
    setInitialCount(0);
    setIndex(0);
    setFeedback(undefined);
    setError(undefined);
  }

  async function play(id: string, slow: boolean) {
    setError(undefined);
    try {
      await audio.play(id, slow);
    } catch (cause) {
      Logger.withError(cause).error('tamil.audio.play_failed');
      setError('Couldn’t play the recording. Try again.');
    }
  }

  return {
    answers, exercise, feedback, preview, pending, error, play,
    number: initialCount + index + 1,
    total: initialCount + round.length,
    begin: () => { audio.stop(); setPreview(false); },
    check: (answer: Answer) => record(answer),
    skip: () => record(),
    advance: () => protect(advance),
    review: () => restart(true),
    restart: () => restart(false),
  };
}
