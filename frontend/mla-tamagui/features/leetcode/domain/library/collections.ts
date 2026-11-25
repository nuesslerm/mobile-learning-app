import type { LibraryContentPort } from './library';

export function collectionRows(content: LibraryContentPort, name: string) {
  if (name === 'Algorithms' || name === 'All Topics') return content.rows;
  if (name === 'NeetCode 150') {
    const rows = new Map(content.rows.map((row) => [row.id, row]));
    return content.studyPlan.flatMap(({ id }) => rows.get(id) ?? []);
  }
  const matches = new Set(content.studyPlan.filter((entry) => entry.group === name || entry.topics.includes(name)).map(({ id }) => id));
  return content.rows.filter((row) => matches.has(row.id) || content.problem(row.id)?.topics.includes(name)).sort((a, b) => a.number - b.number);
}
