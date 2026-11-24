import { z } from 'zod';

import { EMPTY_COURSE, updateCourse } from '../../domain/lessons/course';
import type { CourseProgressPort } from '../../domain/lessons/ports';
import type { SnapshotStorage } from './snapshotStorage';

const KEY = 'tamil-course-v1';
const schema = z.object({
  tasks: z.record(z.string(), z.object({
    answers: z.record(z.string(), z.enum(['correct', 'missed', 'skipped'])),
    completed: z.boolean(),
  })),
  active: z.record(z.string(), z.string()),
});

export function createCourseProgress(storage: SnapshotStorage): CourseProgressPort {
  let queue: Promise<unknown> = Promise.resolve();

  async function read() {
    const raw = await storage.getItem(KEY);
    return raw ? schema.parse(JSON.parse(raw)) : EMPTY_COURSE;
  }

  return {
    async read() {
      await queue;
      return read();
    },
    update(update) {
      const pending = queue.then(async () => {
        const progress = updateCourse(await read(), update);
        await storage.setItem(KEY, JSON.stringify(progress));
        return progress;
      });
      queue = pending.catch(() => undefined);
      return pending;
    },
  };
}
