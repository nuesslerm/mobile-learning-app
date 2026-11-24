import { describe, expect, it } from 'vitest';

import { createCourseProgress } from './courseProgress';
import { createSnapshotStorage } from './snapshotStorage';

describe('Tamil course persistence', () => {
  const target = { topicId: 'colors', taskId: 'colors-1' } as const;

  it('Given concurrent answers and a retried save, When reopened, Then each note keeps its outcome once', async () => {
    const files = new Map<string, string>();
    const backend = {
      async getItem(key: string) { return files.get(key) ?? null; },
      async setItem(key: string, value: string) { files.set(key, value); },
      async removeItem(key: string) { files.delete(key); },
    };
    const progress = createCourseProgress(createSnapshotStorage(backend));
    await Promise.all([
      progress.update({ ...target, kind: 'answer', exerciseId: 'red', outcome: 'correct' }),
      progress.update({ ...target, kind: 'answer', exerciseId: 'green', outcome: 'missed' }),
      progress.update({ ...target, kind: 'answer', exerciseId: 'green', outcome: 'missed' }),
    ]);
    expect(await createCourseProgress(createSnapshotStorage(backend)).read()).toEqual({
      active: { colors: 'colors-1' }, tasks: { 'colors-1': { answers: { red: 'correct', green: 'missed' }, completed: false } },
    });
  });

  it('Given a partial write, When reopened, Then the previous valid snapshot is available and retry succeeds', async () => {
    const files = new Map<string, string>();
    let interrupt = false;
    const backend = {
      async getItem(key: string) { return files.get(key) ?? null; },
      async setItem(key: string, value: string) {
        files.set(key, interrupt ? value.slice(0, 10) : value);
        if (interrupt) throw new Error('Interrupted write');
      },
      async removeItem(key: string) { files.delete(key); },
    };
    const progress = createCourseProgress(createSnapshotStorage(backend));
    await progress.update({ ...target, kind: 'answer', exerciseId: 'red', outcome: 'correct' });
    interrupt = true;
    await expect(progress.update({ ...target, kind: 'complete' })).rejects.toThrow('Interrupted write');
    const recovered = createCourseProgress(createSnapshotStorage(backend));
    expect((await recovered.read()).tasks[target.taskId]?.completed).toBe(false);
    interrupt = false;
    await recovered.update({ ...target, kind: 'complete' });
    expect((await recovered.read()).tasks[target.taskId]?.completed).toBe(true);
  });
});
