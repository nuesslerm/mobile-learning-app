import { normalizeAnswer, type Exercise } from './course';

export type Tile = { id: number; word: string };

export function tilesFor(exercise: Exercise): readonly Tile[] {
  const answer = exercise.kind === 'fill' ? [exercise.words[exercise.blank]] : exercise.words;
  const excluded = new Set(answer.map(normalizeAnswer));
  const extras = exercise.distractors.filter((word) => !excluded.has(normalizeAnswer(word)));
  const tiles = [...answer, ...extras].map((word, id) => ({ id, word }));
  let seed = 0;
  for (const digit of exercise.id) seed = (31 * seed + digit.charCodeAt(0)) >>> 0;
  for (let index = tiles.length - 1; index > 0; index -= 1) {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    const swap = seed % (index + 1);
    [tiles[index], tiles[swap]] = [tiles[swap], tiles[index]];
  }
  return tiles;
}
