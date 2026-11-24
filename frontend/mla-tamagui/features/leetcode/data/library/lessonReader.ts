import type { LibraryLesson } from '../../domain/library/library';

const groups: Record<string, () => Record<string, LibraryLesson>> = {
  'arrays-hashing': () => require('./lessons/arrays-hashing.json'),
  'two-pointers': () => require('./lessons/two-pointers.json'),
  'sliding-window': () => require('./lessons/sliding-window.json'),
  'stack': () => require('./lessons/stack.json'),
  'binary-search': () => require('./lessons/binary-search.json'),
  'linked-list': () => require('./lessons/linked-list.json'),
  'trees': () => require('./lessons/trees.json'),
  'heap-priority-queue': () => require('./lessons/heap-priority-queue.json'),
  'backtracking': () => require('./lessons/backtracking.json'),
  'tries': () => require('./lessons/tries.json'),
  'graphs': () => require('./lessons/graphs.json'),
  'advanced-graphs': () => require('./lessons/advanced-graphs.json'),
  '1-d-dynamic-programming': () => require('./lessons/1-d-dynamic-programming.json'),
  '2-d-dynamic-programming': () => require('./lessons/2-d-dynamic-programming.json'),
  'greedy': () => require('./lessons/greedy.json'),
  'intervals': () => require('./lessons/intervals.json'),
  'math-geometry': () => require('./lessons/math-geometry.json'),
  'bit-manipulation': () => require('./lessons/bit-manipulation.json'),
};

export function readLesson(group: string, id: string) {
  return groups[group]?.()[id];
}
