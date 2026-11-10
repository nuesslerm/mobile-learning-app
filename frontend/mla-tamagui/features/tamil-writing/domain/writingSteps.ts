/**
 * How each letter is formed, in words.
 *
 * Written down from a Tamil writer's description of the alphabet, one entry per
 * stroke, in the order the strokes are made. This is the teaching text: the pad
 * shows the letter faintly and these say what the hand does, so a learner can
 * write it as large as they like rather than chasing a dot along a line.
 *
 * The same counts drive the geometry in
 * `scripts/tamil-strokes/extract-upenn-strokes.py` (COUNTS), so a letter whose
 * steps and strokes disagree is a bug in one of the two -- `writingSteps.test.ts`
 * holds them to each other.
 */

export type WritingSteps = {
  /** What the whole letter is, when it helps to say it in one line. */
  readonly summary?: string;
  /** One line per stroke, in writing order. */
  readonly steps: readonly string[];
};

export const WRITING_STEPS: Readonly<Record<string, WritingSteps>> = {
  'uyir-a': {
    steps: [
      'Start at the top-left loop, curve down and around the lower bowl, and glide right into the horizontal bar.',
      'Lift the pen; draw the vertical stem straight down from top to bottom, crossing the horizontal bar.',
    ],
  },
  'uyir-aa': {
    summary: 'அ, then the tail.',
    steps: [
      'Start at the top-left loop, curve down and around the lower bowl, and glide right into the horizontal bar.',
      'Lift the pen; draw the vertical stem straight down, crossing the horizontal bar.',
      'Lift the pen; from the bottom of the stem, sweep below the baseline into a loop that curls up to the right.',
    ],
  },
  'uyir-i': {
    steps: [
      'Start at the top-left loop, arch over to the right, plunge straight down, sweep across the baseline, and hook upward and inward into a curl.',
    ],
  },
  'uyir-ii': {
    steps: [
      'Left vertical stem straight down.',
      'Top horizontal bar extending right, turning 90° down into the right vertical stem.',
      'Dot inside the enclosure.',
      'Dot outside, centred to the right.',
    ],
  },
  'uyir-u': {
    steps: [
      'Start at the top-left loop, curve down into an open bowl, and sweep flat across the baseline to the right.',
    ],
  },
  'uyir-uu': {
    summary: 'உ, then ள beside it.',
    steps: [
      'Write உ: top-left loop, down into the bowl, flat across the baseline.',
      'Alongside it write ள in one motion: loop, arch, baseline sweep, then the vertical up.',
    ],
  },
  'uyir-e': {
    steps: [
      'Start at the lower-left loop, draw the vertical line straight up, and turn 90° right along the top bar.',
      'Lift the pen; draw the right vertical stem straight down to the baseline.',
    ],
  },
  'uyir-ee': {
    summary: 'எ, then the tail.',
    steps: [
      'Start at the lower-left loop, up the vertical, and turn right along the top bar.',
      'Lift the pen; draw the right vertical stem straight down to the baseline.',
      'Lift the pen; draw a diagonal tail slanting down-left from the bottom-left corner.',
    ],
  },
  'uyir-ai': {
    steps: [
      'Draw the double-arch crest on the left, loop down into an inner twist, sweep flat across the baseline, and flick upward on the right.',
    ],
  },
  'uyir-o': {
    steps: [
      'Start at the top-left loop, curve down, loop back up toward the centre, and sweep the bottom bowl down and across the baseline.',
    ],
  },
  'uyir-oo': {
    summary: 'ஒ, closed with a knot.',
    steps: [
      'The same motion as ஒ, but at the end of the baseline sweep, loop upward and close it into a small knot.',
    ],
  },
  'uyir-au': {
    summary: 'ஒ, then ள beside it.',
    steps: [
      'Write ஒ: top-left loop, down, back up through the middle, bowl across the baseline.',
      'Alongside it write ள in one motion.',
    ],
  },
  aaytham: {
    summary: 'Three taps in an upward triangle.',
    steps: [
      'One dot centred on top.',
      'One dot at the left of the base.',
      'One dot at the right of the base.',
    ],
  },
  'uyirmei-k-a': {
    steps: [
      'Left vertical stem straight down.',
      'Lift the pen; top bar right, drop down, loop left to touch the stem, then kick down-right to the baseline.',
    ],
  },
  'uyirmei-ng-a': {
    steps: [
      'Left stem down, top bar right, right stem down — an inverted "U".',
      'Lift the pen; horizontal foot extending right from the base, hooking down.',
    ],
  },
  'uyirmei-c-a': {
    steps: [
      'Left vertical stem straight down.',
      'Lift the pen; top bar right, drop down on the right, then return flat along the baseline to meet the left stem.',
    ],
  },
  'uyirmei-nj-a': {
    steps: [
      'Top-left loop and arch, like the head of எ.',
      'Lift the pen; the centre connector sweeping around the bottom and ending in an upward hook.',
    ],
  },
  'uyirmei-tt-a': {
    steps: ['Drop straight down, then turn 90° right along the baseline — an "L".'],
  },
  'uyirmei-nn-a': {
    summary: 'The three-loop ṇa.',
    steps: [
      'Bottom-left loop, first arch, second arch, third arch, baseline sweep, then straight up on the right.',
    ],
  },
  'uyirmei-t-a': {
    steps: [
      'Left vertical stem straight down.',
      'Lift the pen; top bar right, drop down curving inward, and sweep down past the baseline into an open tail.',
    ],
  },
  'uyirmei-n-dental-a': {
    steps: [
      'Left vertical stem straight down.',
      'Lift the pen; top bar right, drop down, then hook left along the baseline and sweep down.',
    ],
  },
  'uyirmei-p-a': {
    steps: ['Drop straight down on the left, glide across the baseline, and pull straight up on the right — a square "U".'],
  },
  'uyirmei-m-a': {
    steps: [
      'Draw the square cup of ப: down, across, up.',
      'Lift the pen; draw a horizontal crossbar straight from right to left across the middle.',
    ],
  },
  'uyirmei-y-a': {
    steps: ['First open curve, second open curve, baseline sweep, then straight up on the right.'],
  },
  'uyirmei-r-a': {
    steps: ['Drop straight down on the left, glide across the top, and drop straight down on the right — an inverted "U".'],
  },
  'uyirmei-l-a': {
    steps: [
      'Start at the bottom-left loop, arch into the first crest, dip slightly, arch into the second crest, and drop straight down to the baseline.',
    ],
  },
  'uyirmei-v-a': {
    steps: ['Start at the bottom-left loop, arch over, drop to the baseline, sweep right, and pull straight up.'],
  },
  'uyirmei-zh-a': {
    steps: [
      'Draw the square cup of ப: down, across, up.',
      'Lift the pen; from the mid-left, drop below the baseline, loop under, and curl upward to the right.',
    ],
  },
  'uyirmei-ll-a': {
    steps: [
      'Start at the bottom-left loop, arch over, drop to the baseline, sweep into a rounded bottom belly, and pull straight up.',
    ],
  },
  'uyirmei-rr-a': {
    steps: ['A continuous zigzag: slant up-right, drop down-right, slant up-right, drop down-right to the baseline.'],
  },
  'uyirmei-n-alveolar-a': {
    summary: 'The two-loop ṉa.',
    steps: [
      'Bottom-left loop, first arch, second arch, baseline sweep, then straight up on the right.',
    ],
  },
};

export function writingSteps(characterId: string): WritingSteps | undefined {
  return WRITING_STEPS[characterId];
}

/**
 * A combination's steps: the consonant's own, then what the vowel sign adds.
 * The signs are described once here rather than 216 times.
 */
export const VOWEL_SIGN_STEPS: Readonly<Record<string, string>> = {
  a: '',
  aa: 'Then the ா stroke to its right: down from the top, with the bar across the top.',
  i: 'Then the ி hook riding on its shoulder.',
  ii: 'Then the ீ hook on its shoulder, with the extra curl.',
  u: 'Then the ு tail, joined to the letter at the bottom right.',
  uu: 'Then the ூ tail, joined at the bottom right and curled further.',
  e: 'First the ெ mark to its left, then the letter.',
  ee: 'First the ே mark to its left, then the letter.',
  ai: 'First the ை mark to its left, then the letter.',
  o: 'First the ெ mark to its left, then the letter, then the ா stroke to its right.',
  oo: 'First the ே mark to its left, then the letter, then the ா stroke to its right.',
  au: 'First the ெ mark to its left, then the letter, then the ௌ tail to its right.',
};
