export { curriculum as tamilTestCurriculum } from '../data/lessons/curriculum';
export { createCourseProgress } from '../data/progress/courseProgress';
export type { TamilWritingPorts } from '../domain/ports';

export function createMemoryTamilStorage() {
  const values = new Map<string, string>();
  return {
    async getItem(key: string) { return values.get(key) ?? null; },
    async setItem(key: string, value: string) { values.set(key, value); },
    async removeItem(key: string) { values.delete(key); },
  };
}
