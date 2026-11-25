export type Exercise = {
  id: string;
  sourceGuid: string;
  english: string;
  tamil: string;
  transliteration: string;
  words: readonly string[];
  distractors: readonly string[];
} & ({ kind: 'translate' | 'listen' } | { kind: 'fill'; blank: number });

export type Task = {
  id: string;
  number: number;
  title: string;
  difficulty: 'Easy' | 'Medium' | 'Hard';
  exercises: readonly Exercise[];
};

export type Topic = { id: string; title: string; tasks: readonly Task[] };
export type Section = { id: string; title: string; topics: readonly Topic[] };
export function exerciseCount(topics: readonly Topic[]) {
  return topics.reduce((total, topic) =>
    total + topic.tasks.reduce((count, task) => count + task.exercises.length, 0), 0);
}

export type Answer = string | readonly string[];
export type Verdict = { correct: boolean; expected: string };
export type Outcome = 'correct' | 'missed' | 'skipped';
export type TaskProgress = { answers: Record<string, Outcome>; completed: boolean };
export type CourseProgress = { tasks: Record<string, TaskProgress>; active: Record<string, string> };

export const EMPTY_COURSE: CourseProgress = { tasks: {}, active: {} };

export function normalizeAnswer(value: string) {
  return value.normalize('NFC').trim().replace(/\s+/g, ' ').replace(/\s*[,.;!?…]+(?=\s|$)/g, '');
}

export function checkAnswer(exercise: Pick<Exercise, 'tamil' | 'words'> & ({ kind: 'translate' | 'listen' } | { kind: 'fill'; blank: number }), answer: Answer): Verdict {
  const expected = exercise.kind === 'fill' ? exercise.words[exercise.blank] : exercise.tamil;
  const submitted = normalizeAnswer(typeof answer === 'string' ? answer : answer.join(' '));
  return { correct: submitted !== '' && submitted === normalizeAnswer(expected), expected };
}

export function nextTask(tasks: readonly Task[], progress: CourseProgress['tasks'], active?: string) {
  return tasks.find((task) => task.id === active && !progress[task.id]?.completed)
    ?? tasks.find((task) => !progress[task.id]?.completed);
}

export function matchesTask(task: Task, query: string) {
  const needle = query.normalize('NFC').trim().toLowerCase();
  return !needle || [task.title, ...task.exercises.flatMap((item) => [item.english, item.tamil, item.transliteration])]
    .some((value) => value.normalize('NFC').toLowerCase().includes(needle));
}

export type CourseUpdate = {
  topicId: string;
  taskId: string;
} & ({ kind: 'open' | 'complete' } | { kind: 'answer'; exerciseId: string; outcome: Outcome });

export function updateCourse(progress: CourseProgress, update: CourseUpdate): CourseProgress {
  const active = { ...progress.active, [update.topicId]: update.taskId };
  if (update.kind === 'open') return { ...progress, active };
  const existing = progress.tasks[update.taskId] ?? { answers: {}, completed: false };
  const task = update.kind === 'answer'
    ? { ...existing, answers: { ...existing.answers, [update.exerciseId]: update.outcome } }
    : { ...existing, completed: true };
  return { active, tasks: { ...progress.tasks, [update.taskId]: task } };
}

export function reviewExercises(task: Task, progress?: TaskProgress) {
  return task.exercises.filter((exercise) => {
    const outcome = progress?.answers[exercise.id];
    return outcome === 'missed' || outcome === 'skipped';
  });
}
