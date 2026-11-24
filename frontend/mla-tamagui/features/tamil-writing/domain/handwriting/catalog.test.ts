import { describe, expect, it } from 'vitest';

import { TAMIL_CHARACTERS } from '../lessons';
import { getWritingSubject } from '../tracks';
import { characterGroups } from './catalog';

describe('Handwriting catalog', () => {
  it('Given the Tamil alphabet, When characters are grouped, Then every distinct character opens its own pad', () => {
    const ids = characterGroups.flatMap((group) => group.characters.map(({ id }) => id));
    expect(ids).toHaveLength(251);
    expect(new Set(ids)).toEqual(new Set(TAMIL_CHARACTERS.map(({ id }) => id)));
    expect(ids.every((id) => getWritingSubject(id)?.id === id)).toBe(true);
    expect(characterGroups.filter(({ id }) => id.startsWith('series-')).every(({ characters }) => characters.length === 12)).toBe(true);
  });
});
