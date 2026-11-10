import {
  TAMIL_BASE_CHARACTERS,
  type TamilCharacter,
  type TamilExample,
} from './characters';

/**
 * உயிர்மெய்: the 216 consonant-vowel letters, generated rather than typed.
 *
 * Every one is a consonant carrying a vowel, written as the consonant's base
 * letter plus that vowel's sign. That rule IS the lesson -- eighteen
 * consonants times twelve vowels, and a learner who has written one series has
 * the pattern for the other seventeen -- so the table is derived from it. A
 * hand-written table would be 216 chances to mistype a glyph and no guarantee
 * the rule and the data agree.
 *
 * With the 12 vowels, the 18 bare consonants and ஃ, this makes the classical
 * 247. The grantha letters stay uncombined here: they are borrowed for
 * loanwords, and their series are not part of that count.
 */

export const VIRAMA = '்';

export type VowelSign = {
  /** Matches the uyir character's id suffix, so ids stay derivable. */
  key: string;
  /** The combining sign appended to the consonant. Empty for the inherent அ. */
  sign: string;
  transliteration: string;
  /** The independent vowel, for naming the combination. */
  vowel: string;
  /** How the sign attaches, written from the learner's point of view. */
  hint: string;
};

export const VOWEL_SIGNS: readonly VowelSign[] = [
  {
    key: 'a',
    sign: '',
    transliteration: 'a',
    vowel: 'அ',
    hint: 'the bare consonant without its புள்ளி -- the dot is what the vowel replaces',
  },
  {
    key: 'aa',
    sign: 'ா',
    transliteration: 'ā',
    vowel: 'ஆ',
    hint: 'the ா lengthening stroke to its right',
  },
  {
    key: 'i',
    sign: 'ி',
    transliteration: 'i',
    vowel: 'இ',
    hint: 'the ி hook riding on the shoulder',
  },
  {
    key: 'ii',
    sign: 'ீ',
    transliteration: 'ī',
    vowel: 'ஈ',
    hint: 'the taller ீ curl above -- one loop more than ி',
  },
  {
    key: 'u',
    sign: 'ு',
    transliteration: 'u',
    vowel: 'உ',
    hint: 'the ு sign, which FUSES with the letter rather than sitting beside it -- its shape differs per consonant, so follow the guide, not a rule',
  },
  {
    key: 'uu',
    sign: 'ூ',
    transliteration: 'ū',
    vowel: 'ஊ',
    hint: 'the ூ sign, fused like ு with one more curl',
  },
  {
    key: 'e',
    sign: 'ெ',
    transliteration: 'e',
    vowel: 'எ',
    hint: 'the ெ sign, written FIRST, to the left',
  },
  {
    key: 'ee',
    sign: 'ே',
    transliteration: 'ē',
    vowel: 'ஏ',
    hint: 'the taller ே sign on the left',
  },
  {
    key: 'ai',
    sign: 'ை',
    transliteration: 'ai',
    vowel: 'ஐ',
    hint: 'the ை sign on the left, wider than ே',
  },
  {
    key: 'o',
    sign: 'ொ',
    transliteration: 'o',
    vowel: 'ஒ',
    hint: 'a two-part vowel: ெ on the left, ா on the right',
  },
  {
    key: 'oo',
    sign: 'ோ',
    transliteration: 'ō',
    vowel: 'ஓ',
    hint: 'two-part like ொ, with the taller ே on the left',
  },
  {
    key: 'au',
    sign: 'ௌ',
    transliteration: 'au',
    vowel: 'ஔ',
    hint: 'two-part: ெ on the left, the ௌ tail on the right',
  },
];

/** The eighteen மெய், in the order the alphabet recites them. */
export const CONSONANT_ORDER: readonly string[] = [
  'mei-k',
  'mei-ng',
  'mei-c',
  'mei-nj',
  'mei-tt',
  'mei-nn',
  'mei-t',
  'mei-n-dental',
  'mei-p',
  'mei-m',
  'mei-y',
  'mei-r',
  'mei-l',
  'mei-v',
  'mei-zh',
  'mei-ll',
  'mei-rr',
  'mei-n-alveolar',
];

export type TamilConsonant = {
  /** The மெய் character's id, e.g. `mei-k`. */
  meiId: string;
  /** That id without its `mei-` prefix; the uyirmei id and lesson id use it. */
  key: string;
  /** The consonant with its புள்ளி, e.g. க். */
  meiGlyph: string;
  /** The bare letter the vowel signs attach to, e.g. க. */
  base: string;
  transliteration: string;
  tamilName: string;
};

const BASE_BY_ID = new Map(
  TAMIL_BASE_CHARACTERS.map((character) => [character.id, character])
);

export const TAMIL_CONSONANTS: readonly TamilConsonant[] = CONSONANT_ORDER.map(
  (meiId) => {
    const mei = BASE_BY_ID.get(meiId);
    if (!mei) {
      throw new Error(`uyirmei: no மெய் character with id "${meiId}"`);
    }
    return {
      meiId,
      key: meiId.replace(/^mei-/, ''),
      meiGlyph: mei.glyph,
      // The bare letter is the மெய் without its virama -- derived rather than
      // restated, so the two can never drift apart.
      base: mei.glyph.replace(VIRAMA, ''),
      transliteration: mei.transliteration,
      tamilName: mei.tamilName,
    };
  }
);

/**
 * Example words, by generated id. Only the ones worth a learner's attention --
 * an example per combination would be 216 words, most of them contrived.
 */
const EXAMPLES: Readonly<Record<string, TamilExample>> = {
  'uyirmei-k-a': { word: 'கடல்', transliteration: 'kaṭal', meaning: 'sea' },
  'uyirmei-k-aa': { word: 'காது', transliteration: 'kātu', meaning: 'ear' },
  'uyirmei-k-i': { word: 'கிளி', transliteration: 'kiḷi', meaning: 'parrot' },
  'uyirmei-k-ii': { word: 'கீரை', transliteration: 'kīrai', meaning: 'greens' },
  'uyirmei-k-u': { word: 'குதிரை', transliteration: 'kutirai', meaning: 'horse' },
  'uyirmei-k-uu': { word: 'கூடை', transliteration: 'kūṭai', meaning: 'basket' },
  'uyirmei-k-e': { word: 'கெண்டை', transliteration: 'keṇṭai', meaning: 'carp' },
  'uyirmei-k-ee': { word: 'கேள்வி', transliteration: 'kēḷvi', meaning: 'question' },
  'uyirmei-k-ai': { word: 'கை', transliteration: 'kai', meaning: 'hand' },
  'uyirmei-k-o': { word: 'கொடி', transliteration: 'koṭi', meaning: 'flag' },
  'uyirmei-k-oo': { word: 'கோழி', transliteration: 'kōḻi', meaning: 'hen' },
  'uyirmei-k-au': { word: 'கௌரவம்', transliteration: 'kauravam', meaning: 'honour' },
  'uyirmei-c-a': { word: 'சட்டை', transliteration: 'caṭṭai', meaning: 'shirt' },
  'uyirmei-t-a': { word: 'தலை', transliteration: 'talai', meaning: 'head' },
  'uyirmei-p-a': { word: 'பல்', transliteration: 'pal', meaning: 'tooth' },
  'uyirmei-m-a': { word: 'மரம்', transliteration: 'maram', meaning: 'tree' },
  'uyirmei-v-a': { word: 'வலை', transliteration: 'valai', meaning: 'net' },
  'uyirmei-n-alveolar-ai': { word: 'நாய்', transliteration: 'nāy', meaning: 'dog' },
};

export function uyirmeiId(consonantKey: string, vowelKey: string): string {
  return `uyirmei-${consonantKey}-${vowelKey}`;
}

function buildCharacter(
  consonant: TamilConsonant,
  vowel: VowelSign
): TamilCharacter {
  const id = uyirmeiId(consonant.key, vowel.key);
  const example = EXAMPLES[id];
  return {
    id,
    glyph: `${consonant.base}${vowel.sign}`,
    kind: 'uyirmei',
    transliteration: `${consonant.transliteration}${vowel.transliteration}`,
    tamilName: `${consonant.meiGlyph} + ${vowel.vowel}`,
    sound: `${consonant.transliteration} carrying ${vowel.transliteration}`,
    strokeHint: `${consonant.base} with ${vowel.hint}.`,
    ...(example ? { example } : {}),
  };
}

/** The 216, consonant by consonant, vowels in alphabet order within each. */
export const TAMIL_UYIRMEI: readonly TamilCharacter[] = TAMIL_CONSONANTS.flatMap(
  (consonant) => VOWEL_SIGNS.map((vowel) => buildCharacter(consonant, vowel))
);

/** The ids of one consonant's twelve, in order. */
export function seriesCharacterIds(consonantKey: string): string[] {
  return VOWEL_SIGNS.map((vowel) => uyirmeiId(consonantKey, vowel.key));
}
