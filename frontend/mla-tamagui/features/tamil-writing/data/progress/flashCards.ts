import { preview, type CardReview, type Direction, type FlashCardProgressPort } from '../../domain/lessons/flashCards';
import type { SnapshotStorage } from './snapshotStorage';

const prefix = 'tamil-flash-v1-';
const directionKey = prefix + 'direction';

export function createFlashCardProgress(storage: SnapshotStorage): FlashCardProgressPort {
  let queue: Promise<unknown> = Promise.resolve();
  const key = (id: string, direction: Direction) => prefix + direction + '-' + id;
  async function readCard(id: string, direction: Direction): Promise<CardReview | undefined> {
    const raw = await storage.getItem(key(id, direction));
    return raw ? JSON.parse(raw) : undefined;
  }
  return {
    async readDirection() {
      return await storage.getItem(directionKey) === 'tamil-first' ? 'tamil-first' : 'english-first';
    },
    setDirection: (direction) => storage.setItem(directionKey, direction),
    async read(ids, direction) {
      await queue;
      const entries = await Promise.all(ids.map(async (id) => {
        const review = await readCard(id, direction);
        return review ? [[id, review] as const] : [];
      }));
      return Object.fromEntries(entries.flat());
    },
    rate(id, direction, reps, grade, now) {
      const pending = queue.then(async () => {
        const previous = await readCard(id, direction);
        if (previous && previous.card.reps !== reps) return previous;
        const { card, log } = preview(previous, now)[grade];
        const review = { card, history: [...previous?.history ?? [], log] };
        await storage.setItem(key(id, direction), JSON.stringify(review));
        return review;
      });
      queue = pending.catch(() => undefined);
      return pending;
    },
  };
}
