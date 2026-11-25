import { checkAnswer, type Section } from '../../domain/lessons/course';
import type { CurriculumPort } from '../../domain/lessons/ports';
import course from './course.json';

const sections = course.sections as readonly Section[];
const tasks = new Map(sections.flatMap((section) => section.topics.flatMap((topic) =>
  topic.tasks.map((task) => [task.id, task] as const)
)));

export const curriculum = {
  async listSections() {
    return sections;
  },
  async check(taskId, exerciseId, answer) {
    const exercise = tasks.get(taskId)?.exercises.find((item) => item.id === exerciseId);
    if (!exercise) throw new Error('Tamil exercise not found');
    return checkAnswer(exercise, answer);
  },
} satisfies CurriculumPort;
