import { TAMIL_BASE_CHARACTERS, type TamilCharacter } from './characters';
import { TAMIL_CHARACTERS } from './lessons';
import { TAMIL_UYIRMEI } from './uyirmei';
import { VOWEL_SIGN_STEPS, writingSteps } from './writingSteps';

/**
 * Tracks: what there is to practise writing, grouped the way a learner picks.
 *
 * The lessons in `lessons.ts` are the course order for tracing one letter at a
 * time. A track is looser -- a set of things to write large on the pad, from
 * the shape and the instructions, and have marked: the twelve vowels, the
 * consonants, a series of combinations, or words. Words are the letters'
 * own example words, so nothing here is invented: every word is one a letter
 * card already teaches, and its letters are letters the course already has.
 */

type TrackKind = 'letters' | 'words';

export type WritingSubject = {
  /** Stable id, usable in a route: a character id, or `word-<slug>`. */
  id: string;
  kind: 'letter' | 'word';
  /** What goes on the pad as the template. */
  glyph: string;
  transliteration: string;
  /** The letter's Tamil name, or the word's meaning. */
  label: string;
  /** One line per stroke for a letter; one line per letter for a word. */
  steps: readonly string[];
  /** For a word: the ids of its letters the course knows, in order. */
  characterIds: readonly string[];
  /** A letter's own stroke geometry exists for this id, so an outline can be drawn. */
  hasGuideId?: string;
};

export type Track = {
  id: string;
  kind: TrackKind;
  title: string;
  subtitle: string;
  subjects: readonly WritingSubject[];
};

function letterSubject(character: TamilCharacter): WritingSubject {
  const own = writingSteps(character.id);
  let steps: readonly string[];
  if (own) {
    steps = own.steps;
  } else if (character.id.startsWith('uyirmei-')) {
    // A combination: the consonant's steps, then what the sign adds.
    const parts = character.id.split('-');
    const vowel = parts[parts.length - 1] ?? '';
    const base = writingSteps(`${parts.slice(0, -1).join('-')}-a`);
    const sign = VOWEL_SIGN_STEPS[vowel];
    steps = [...(base?.steps ?? []), ...(sign ? [sign] : [])];
  } else {
    steps = [character.strokeHint];
  }
  return {
    id: character.id,
    kind: 'letter',
    glyph: character.glyph,
    transliteration: character.transliteration,
    label: character.tamilName,
    steps,
    characterIds: [character.id],
    hasGuideId: character.id,
  };
}

/**
 * Splits a Tamil word into the course's letters. Combining marks belong to
 * the consonant before them, so `அம்மா` is அ · ம் · மா, not five code points.
 */
export function splitWord(word: string): string[] {
  const out: string[] = [];
  for (const ch of Array.from(word)) {
    const code = ch.codePointAt(0) ?? 0;
    // Tamil vowel signs, virama and the au-length mark: U+0BBE–U+0BCD, U+0BD7.
    const combining = (code >= 0x0bbe && code <= 0x0bcd) || code === 0x0bd7;
    if (combining && out.length > 0) out[out.length - 1] += ch;
    else out.push(ch);
  }
  return out;
}

const BY_GLYPH = new Map<string, TamilCharacter>();
for (const character of TAMIL_CHARACTERS) BY_GLYPH.set(character.glyph, character);

function wordSubject(character: TamilCharacter): WritingSubject | undefined {
  const example = character.example;
  if (!example) return undefined;
  const letters = splitWord(example.word);
  const known = letters.map((glyph) => BY_GLYPH.get(glyph));
  return {
    id: `word-${character.id}`,
    kind: 'word',
    glyph: example.word,
    transliteration: example.transliteration,
    label: example.meaning,
    steps: letters.map((glyph, index) => {
      const letter = known[index];
      // The pad numbers the steps itself.
      return letter ? `${glyph} — ${letter.transliteration} (${letter.tamilName})` : glyph;
    }),
    characterIds: known.flatMap((letter) => (letter ? [letter.id] : [])),
  };
}

const VOWELS = TAMIL_BASE_CHARACTERS.filter((c) => c.kind === 'uyir');
const CONSONANTS = TAMIL_UYIRMEI.filter((c) => c.id.endsWith('-a'));
const OTHERS = TAMIL_BASE_CHARACTERS.filter((c) => c.kind !== 'uyir' && c.kind !== 'mei');

const LETTER_WORDS = VOWELS.concat(OTHERS)
  .map(wordSubject)
  .filter((s): s is WritingSubject => s !== undefined);
const COMBINATION_WORDS = TAMIL_UYIRMEI.map(wordSubject).filter(
  (s): s is WritingSubject => s !== undefined
);

const ALL_TRACKS: Track[] = [
  {
    id: 'vowels',
    kind: 'letters',
    title: 'உயிர் எழுத்து',
    subtitle: 'The twelve vowels',
    subjects: VOWELS.map(letterSubject),
  },
  {
    id: 'consonants',
    kind: 'letters',
    title: 'மெய் எழுத்து',
    subtitle: 'The eighteen consonants',
    subjects: CONSONANTS.map(letterSubject),
  },
  {
    id: 'other-letters',
    kind: 'letters',
    title: 'ஆய்தம் · கிரந்தம்',
    subtitle: 'ஃ and the grantha letters',
    subjects: OTHERS.map(letterSubject),
  },
  {
    id: 'combinations',
    kind: 'letters',
    title: 'உயிர்மெய்',
    subtitle: 'All 216 consonant + vowel combinations',
    subjects: TAMIL_UYIRMEI.map(letterSubject),
  },
  {
    id: 'first-words',
    kind: 'words',
    title: 'முதல் சொற்கள்',
    subtitle: 'First words, one per vowel',
    subjects: LETTER_WORDS,
  },
  {
    id: 'more-words',
    kind: 'words',
    title: 'மேலும் சொற்கள்',
    subtitle: 'Words built on the combinations',
    subjects: COMBINATION_WORDS,
  },
];

export const TRACKS: readonly Track[] = ALL_TRACKS.filter((track) => track.subjects.length > 0);

export function getTrack(trackId: string): Track | undefined {
  return TRACKS.find((track) => track.id === trackId);
}

export function getWritingSubject(subjectId: string): WritingSubject | undefined {
  for (const track of TRACKS) {
    const found = track.subjects.find((subject) => subject.id === subjectId);
    if (found) return found;
  }
  return undefined;
}

/** The track a subject sits in, and its neighbours, for "next" on the pad. */
export function subjectPosition(
  subjectId: string
): { track: Track; index: number } | undefined {
  for (const track of TRACKS) {
    const index = track.subjects.findIndex((subject) => subject.id === subjectId);
    if (index !== -1) return { track, index };
  }
  return undefined;
}
