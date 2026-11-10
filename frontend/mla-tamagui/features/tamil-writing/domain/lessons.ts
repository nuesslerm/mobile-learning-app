import { TAMIL_BASE_CHARACTERS, type TamilCharacter } from './characters';
import {
  TAMIL_CONSONANTS,
  TAMIL_UYIRMEI,
  seriesCharacterIds,
} from './uyirmei';

/**
 * The course order: the vowels, the consonants by class, ஃ and the grantha
 * letters, then one lesson per consonant series -- eighteen of them, twelve
 * letters each, which is the whole உயிர்மெய் grid.
 *
 * Sections exist because 24 lessons in one flat list is a scroll, not a
 * curriculum: a learner should see at a glance that the last eighteen are all
 * the same exercise with a different consonant.
 */

const LESSON_SECTIONS = ['uyir', 'mei', 'other', 'uyirmei'] as const;

type LessonSection = (typeof LESSON_SECTIONS)[number];

export type TamilLesson = {
  id: string;
  section: LessonSection;
  /** Lesson title in Tamil. */
  title: string;
  /** The same title in English, for learners who cannot yet read the script. */
  subtitle: string;
  description: string;
  characterIds: readonly string[];
};

const SECTION_TITLES: Record<LessonSection, string> = {
  uyir: 'உயிர் · Vowels',
  mei: 'மெய் · Consonants',
  other: 'ஆய்தம் · Other letters',
  uyirmei: 'உயிர்மெய் · Combinations',
};

const BASE_LESSONS: readonly TamilLesson[] = [
  {
    id: 'uyir-1',
    section: 'uyir',
    title: 'உயிர் எழுத்து I',
    subtitle: 'Vowels: அ to ஊ',
    description:
      'The first six of the twelve life-letters. Each short vowel is followed by its long partner -- learn them as pairs.',
    characterIds: ['uyir-a', 'uyir-aa', 'uyir-i', 'uyir-ii', 'uyir-u', 'uyir-uu'],
  },
  {
    id: 'uyir-2',
    section: 'uyir',
    title: 'உயிர் எழுத்து II',
    subtitle: 'Vowels: எ to ஔ',
    description:
      'The remaining six vowels, including the two diphthongs ஐ and ஔ.',
    characterIds: ['uyir-e', 'uyir-ee', 'uyir-ai', 'uyir-o', 'uyir-oo', 'uyir-au'],
  },
  {
    id: 'vallinam',
    section: 'mei',
    title: 'வல்லினம்',
    subtitle: 'Hard consonants',
    description:
      'Six body-letters, each written with a புள்ளி (dot) above to mark it as a bare consonant.',
    characterIds: ['mei-k', 'mei-c', 'mei-tt', 'mei-t', 'mei-p', 'mei-rr'],
  },
  {
    id: 'mellinam',
    section: 'mei',
    title: 'மெல்லினம்',
    subtitle: 'Nasal consonants',
    description:
      'The six nasals. Each one is the nasal partner of a வல்லினம் letter you already know.',
    characterIds: [
      'mei-ng',
      'mei-nj',
      'mei-nn',
      'mei-n-dental',
      'mei-m',
      'mei-n-alveolar',
    ],
  },
  {
    id: 'idaiyinam',
    section: 'mei',
    title: 'இடையினம்',
    subtitle: 'Medium consonants',
    description:
      'The six in-between letters, including ழ -- the retroflex sound Tamil is named for.',
    characterIds: ['mei-y', 'mei-r', 'mei-l', 'mei-v', 'mei-zh', 'mei-ll'],
  },
  {
    id: 'aaytham-grantha',
    section: 'other',
    title: 'ஆய்தம் மற்றும் கிரந்தம்',
    subtitle: 'Aaytham and grantha letters',
    description:
      'ஃ, the thirty-first letter, plus the four grantha letters Tamil borrows for loanwords.',
    characterIds: ['aaytham', 'grantha-j', 'grantha-sh', 'grantha-s', 'grantha-h'],
  },
];

/**
 * One lesson per consonant: its twelve vowel forms, in alphabet order. The
 * description names what changes, because what a learner is memorising by the
 * third series is the SIGNS, not the letters.
 */
const SERIES_LESSONS: readonly TamilLesson[] = TAMIL_CONSONANTS.map(
  (consonant, index) => ({
    id: `uyirmei-${consonant.key}`,
    section: 'uyirmei' as const,
    title: `${consonant.base} வரிசை`,
    subtitle: `The ${consonant.transliteration} series`,
    description:
      index === 0
        ? `All twelve vowels on ${consonant.meiGlyph}. The signs behave the same on every consonant, so this series is the pattern for the seventeen that follow -- except ${'ு'} and ${'ூ'}, which fuse into the letter and change shape.`
        : `${consonant.meiGlyph} carrying each of the twelve vowels, in the same order as the ${TAMIL_CONSONANTS[0]?.transliteration ?? 'k'} series.`,
    characterIds: seriesCharacterIds(consonant.key),
  })
);

export const TAMIL_LESSONS: readonly TamilLesson[] = [
  ...BASE_LESSONS,
  ...SERIES_LESSONS,
];

/** Every letter the course teaches: 12 + 18 + ஃ + 4 grantha + 216. */
export const TAMIL_CHARACTERS: readonly TamilCharacter[] = [
  ...TAMIL_BASE_CHARACTERS,
  ...TAMIL_UYIRMEI,
];

const CHARACTERS_BY_ID = new Map<string, TamilCharacter>(
  TAMIL_CHARACTERS.map((character) => [character.id, character])
);

export function getCharacter(id: string): TamilCharacter | undefined {
  return CHARACTERS_BY_ID.get(id);
}

export function getLesson(id: string): TamilLesson | undefined {
  return TAMIL_LESSONS.find((lesson) => lesson.id === id);
}

/**
 * A lesson's characters, resolved. An id with no character behind it is
 * dropped rather than thrown on: a broken id should cost one letter, not the
 * whole screen. `lessonCharacterIdsResolve` is the test that keeps that from
 * hiding a typo.
 */
export function getLessonCharacters(lessonId: string): TamilCharacter[] {
  const lesson = getLesson(lessonId);
  if (!lesson) return [];
  return lesson.characterIds
    .map((id) => CHARACTERS_BY_ID.get(id))
    .filter((character): character is TamilCharacter => character !== undefined);
}

export function lessonCharacterIdsResolve(): boolean {
  return TAMIL_LESSONS.every((lesson) =>
    lesson.characterIds.every((id) => CHARACTERS_BY_ID.has(id))
  );
}

export type LessonGroup = {
  section: LessonSection;
  title: string;
  lessons: TamilLesson[];
};

/** Lessons grouped for the course screen, in course order. */
export function lessonsBySection(): LessonGroup[] {
  return LESSON_SECTIONS.map((section) => ({
    section,
    title: SECTION_TITLES[section],
    lessons: TAMIL_LESSONS.filter((lesson) => lesson.section === section),
  })).filter((group) => group.lessons.length > 0);
}
