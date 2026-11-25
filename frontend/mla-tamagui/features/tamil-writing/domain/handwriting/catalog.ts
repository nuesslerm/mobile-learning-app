import { TAMIL_BASE_CHARACTERS, type TamilCharacter } from '../characters';
import { TAMIL_CONSONANTS, TAMIL_UYIRMEI, seriesCharacterIds } from '../uyirmei';

export type CharacterGroup = { id: string; title: string; characters: readonly TamilCharacter[] };

export const characterGroups: readonly CharacterGroup[] = [
  { id: 'vowels', title: 'Vowels · உயிர்', characters: TAMIL_BASE_CHARACTERS.filter((item) => item.kind === 'uyir') },
  { id: 'consonants', title: 'Pure consonants · மெய்', characters: TAMIL_BASE_CHARACTERS.filter((item) => item.kind === 'mei') },
  ...TAMIL_CONSONANTS.map(({ key, base }) => {
    const ids = new Set(seriesCharacterIds(key));
    const characters = TAMIL_UYIRMEI.filter((item) => ids.has(item.id));
    return { id: `series-${key}`, title: `${base} series · உயிர்மெய்`, characters };
  }),
  { id: 'aaytham', title: 'Aaytham', characters: TAMIL_BASE_CHARACTERS.filter((item) => item.kind === 'aaytham') },
  { id: 'grantha', title: 'Grantha', characters: TAMIL_BASE_CHARACTERS.filter((item) => item.kind === 'grantha') },
];

const orderedCharacters = characterGroups.flatMap(({ characters }) => characters);

export function characterPosition(id: string) {
  const index = orderedCharacters.findIndex((character) => character.id === id);
  return index < 0 ? undefined : { previous: orderedCharacters[index - 1], next: orderedCharacters[index + 1] };
}

export function matchesCharacter(character: TamilCharacter, query: string) {
  const needle = query.trim().normalize('NFC').toLowerCase();
  return [character.glyph, character.transliteration, character.tamilName]
    .some((value) => value.normalize('NFC').toLowerCase().includes(needle));
}
