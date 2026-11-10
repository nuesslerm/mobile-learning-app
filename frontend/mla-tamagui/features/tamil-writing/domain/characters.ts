/**
 * The Tamil letters this course teaches, as pure data.
 *
 * The guide glyph a learner traces is the font's own rendering of `glyph`, so
 * this file never carries outline or stroke-path data -- only what a learner
 * needs to read beside it. `strokeHint` describes the shape in plain words; it
 * is a memory aid, not calligraphic authority.
 */

const CHARACTER_KINDS = [
  'uyir',
  'mei',
  'aaytham',
  'grantha',
  'uyirmei',
] as const;

type TamilCharacterKind = (typeof CHARACTER_KINDS)[number];

export type TamilExample = {
  word: string;
  transliteration: string;
  meaning: string;
};

export type TamilCharacter = {
  /** Stable id; also the progress-store key, so it never changes once shipped. */
  id: string;
  glyph: string;
  kind: TamilCharacterKind;
  /** ISO 15919 transliteration. */
  transliteration: string;
  /** The letter's Tamil name, e.g. அகரம் for அ. */
  tamilName: string;
  /** English approximation of the sound. */
  sound: string;
  /** Shape description to hold in mind while tracing. */
  strokeHint: string;
  example?: TamilExample;
};

/**
 * The base letters, written out. The 216 உயிர்மெய் combinations are not here:
 * they are generated in `uyirmei.ts` from these consonants and the vowel
 * signs, because a hand-written table of 216 rows is 216 chances to mistype a
 * glyph, and the composition rule is the thing a learner is actually being
 * taught.
 */
export const TAMIL_BASE_CHARACTERS: readonly TamilCharacter[] = [
  // --- உயிர் எழுத்து (vowels) ---
  {
    id: 'uyir-a',
    glyph: 'அ',
    kind: 'uyir',
    transliteration: 'a',
    tamilName: 'அகரம்',
    sound: 'short "a", as in about',
    strokeHint: 'Upper hook first, then the long vertical with its foot curling right.',
    example: { word: 'அம்மா', transliteration: 'ammā', meaning: 'mother' },
  },
  {
    id: 'uyir-aa',
    glyph: 'ஆ',
    kind: 'uyir',
    transliteration: 'ā',
    tamilName: 'ஆகாரம்',
    sound: 'long "aa", as in father',
    strokeHint: 'Write அ, then add the tall lengthening stroke on the right.',
    example: { word: 'ஆடு', transliteration: 'āṭu', meaning: 'goat' },
  },
  {
    id: 'uyir-i',
    glyph: 'இ',
    kind: 'uyir',
    transliteration: 'i',
    tamilName: 'இகரம்',
    sound: 'short "i", as in sit',
    strokeHint: 'A small loop at the top, then the wide sweep beneath it.',
    example: { word: 'இலை', transliteration: 'ilai', meaning: 'leaf' },
  },
  {
    id: 'uyir-ii',
    glyph: 'ஈ',
    kind: 'uyir',
    transliteration: 'ī',
    tamilName: 'ஈகாரம்',
    sound: 'long "ee", as in seed',
    strokeHint: 'இ with a second loop stacked above -- keep both loops the same size.',
    example: { word: 'ஈ', transliteration: 'ī', meaning: 'fly (the insect)' },
  },
  {
    id: 'uyir-u',
    glyph: 'உ',
    kind: 'uyir',
    transliteration: 'u',
    tamilName: 'உகரம்',
    sound: 'short "u", as in put',
    strokeHint: 'One continuous stroke: down, round the bowl, out to the right.',
    example: { word: 'உடல்', transliteration: 'uṭal', meaning: 'body' },
  },
  {
    id: 'uyir-uu',
    glyph: 'ஊ',
    kind: 'uyir',
    transliteration: 'ū',
    tamilName: 'ஊகாரம்',
    sound: 'long "oo", as in moon',
    strokeHint: 'உ, then the extra curl trailing off the right shoulder.',
    example: { word: 'ஊர்', transliteration: 'ūr', meaning: 'town' },
  },
  {
    id: 'uyir-e',
    glyph: 'எ',
    kind: 'uyir',
    transliteration: 'e',
    tamilName: 'எகரம்',
    sound: 'short "e", as in bed',
    strokeHint: 'Start mid-height, loop left and close the bowl below.',
    example: { word: 'எலி', transliteration: 'eli', meaning: 'rat' },
  },
  {
    id: 'uyir-ee',
    glyph: 'ஏ',
    kind: 'uyir',
    transliteration: 'ē',
    tamilName: 'ஏகாரம்',
    sound: 'long "ay", as in bay',
    strokeHint: 'எ with the top extended into a flag over the bowl.',
    example: { word: 'ஏணி', transliteration: 'ēṇi', meaning: 'ladder' },
  },
  {
    id: 'uyir-ai',
    glyph: 'ஐ',
    kind: 'uyir',
    transliteration: 'ai',
    tamilName: 'ஐகாரம்',
    sound: '"ai", as in aisle',
    strokeHint: 'Two linked bowls: the small one first, the larger one closing right.',
    example: { word: 'ஐந்து', transliteration: 'aintu', meaning: 'five' },
  },
  {
    id: 'uyir-o',
    glyph: 'ஒ',
    kind: 'uyir',
    transliteration: 'o',
    tamilName: 'ஒகரம்',
    sound: 'short "o", as in pot',
    strokeHint: 'A closed ring on the left, then the tail sweeping right.',
    example: { word: 'ஒட்டகம்', transliteration: 'oṭṭakam', meaning: 'camel' },
  },
  {
    id: 'uyir-oo',
    glyph: 'ஓ',
    kind: 'uyir',
    transliteration: 'ō',
    tamilName: 'ஓகாரம்',
    sound: 'long "oh", as in go',
    strokeHint: 'ஒ, then lift and cap it with the lengthening stroke.',
    example: { word: 'ஓடு', transliteration: 'ōṭu', meaning: 'run' },
  },
  {
    id: 'uyir-au',
    glyph: 'ஔ',
    kind: 'uyir',
    transliteration: 'au',
    tamilName: 'ஔகாரம்',
    sound: '"ow", as in cow',
    strokeHint: 'ஒ on the left, then the tall upright and its curl on the right.',
    example: { word: 'ஔடதம்', transliteration: 'auṭatam', meaning: 'medicine' },
  },

  // --- வல்லினம் (hard consonants) ---
  {
    id: 'mei-k',
    glyph: 'க்',
    kind: 'mei',
    transliteration: 'k',
    tamilName: 'ககரம்',
    sound: 'k / g, as in skate',
    strokeHint: 'Vertical down, shoulder out to the right, then the dot (புள்ளி) on top.',
    example: { word: 'பக்கம்', transliteration: 'pakkam', meaning: 'page, side' },
  },
  {
    id: 'mei-c',
    glyph: 'ச்',
    kind: 'mei',
    transliteration: 'c',
    tamilName: 'சகரம்',
    sound: 'ch / s, as in chair',
    strokeHint: 'One open curve like a backwards C, then the dot above.',
    example: { word: 'பச்சை', transliteration: 'paccai', meaning: 'green' },
  },
  {
    id: 'mei-tt',
    glyph: 'ட்',
    kind: 'mei',
    transliteration: 'ṭ',
    tamilName: 'டகரம்',
    sound: 'retroflex "t" -- tongue curled back',
    strokeHint: 'A tight spiral wound inward, finished with the dot.',
    example: { word: 'கட்டில்', transliteration: 'kaṭṭil', meaning: 'cot' },
  },
  {
    id: 'mei-t',
    glyph: 'த்',
    kind: 'mei',
    transliteration: 't',
    tamilName: 'தகரம்',
    sound: 'dental "t" -- tongue on the teeth',
    strokeHint: 'Loop on the left, bowl to the right, dot on top.',
    example: { word: 'பத்து', transliteration: 'pattu', meaning: 'ten' },
  },
  {
    id: 'mei-p',
    glyph: 'ப்',
    kind: 'mei',
    transliteration: 'p',
    tamilName: 'பகரம்',
    sound: 'p / b, as in spin',
    strokeHint: 'Left vertical, across the top, down the right -- an open box, then the dot.',
    example: { word: 'பாப்பா', transliteration: 'pāppā', meaning: 'baby' },
  },
  {
    id: 'mei-rr',
    glyph: 'ற்',
    kind: 'mei',
    transliteration: 'ṟ',
    tamilName: 'றகரம்',
    sound: 'hard trilled "r"',
    strokeHint: 'Like ர் but with the top hooked further left; dot above.',
    example: { word: 'ஆறு', transliteration: 'āṟu', meaning: 'river, six' },
  },

  // --- மெல்லினம் (nasal consonants) ---
  {
    id: 'mei-ng',
    glyph: 'ங்',
    kind: 'mei',
    transliteration: 'ṅ',
    tamilName: 'ஙகரம்',
    sound: '"ng", as in sing',
    strokeHint: 'A wide bowl with a hook rising from the left, then the dot.',
    example: { word: 'தங்கம்', transliteration: 'taṅkam', meaning: 'gold' },
  },
  {
    id: 'mei-nj',
    glyph: 'ஞ்',
    kind: 'mei',
    transliteration: 'ñ',
    tamilName: 'ஞகரம்',
    sound: '"ny", as in canyon',
    strokeHint: 'Loop, then a long tail curving down and left; dot above.',
    example: { word: 'மஞ்சள்', transliteration: 'mañcaḷ', meaning: 'turmeric, yellow' },
  },
  {
    id: 'mei-nn',
    glyph: 'ண்',
    kind: 'mei',
    transliteration: 'ṇ',
    tamilName: 'ணகரம்',
    sound: 'retroflex "n" -- tongue curled back',
    strokeHint: 'A spiral opening upward, the tail lifting to the right, then the dot.',
    example: { word: 'மண்', transliteration: 'maṇ', meaning: 'soil' },
  },
  {
    id: 'mei-n-dental',
    glyph: 'ந்',
    kind: 'mei',
    transliteration: 'n',
    tamilName: 'நகரம்',
    sound: 'dental "n" -- tongue on the teeth',
    strokeHint: 'Like த் with the bowl left open on the right; dot on top.',
    example: { word: 'பந்து', transliteration: 'pantu', meaning: 'ball' },
  },
  {
    id: 'mei-m',
    glyph: 'ம்',
    kind: 'mei',
    transliteration: 'm',
    tamilName: 'மகரம்',
    sound: 'm, as in moon',
    strokeHint: 'Left vertical, arch over the top, down into the closed bowl; dot above.',
    example: { word: 'மரம்', transliteration: 'maram', meaning: 'tree' },
  },
  {
    id: 'mei-n-alveolar',
    glyph: 'ன்',
    kind: 'mei',
    transliteration: 'ṉ',
    tamilName: 'னகரம்',
    sound: 'alveolar "n" -- the ridge behind the teeth',
    strokeHint: 'A single low bowl opening upward, then the dot.',
    example: { word: 'மீன்', transliteration: 'mīṉ', meaning: 'fish' },
  },

  // --- இடையினம் (medium consonants) ---
  {
    id: 'mei-y',
    glyph: 'ய்',
    kind: 'mei',
    transliteration: 'y',
    tamilName: 'யகரம்',
    sound: 'y, as in yes',
    strokeHint: 'Hook down on the left, then the long stroke swinging right; dot above.',
    example: { word: 'வாய்', transliteration: 'vāy', meaning: 'mouth' },
  },
  {
    id: 'mei-r',
    glyph: 'ர்',
    kind: 'mei',
    transliteration: 'r',
    tamilName: 'ரகரம்',
    sound: 'soft tapped "r"',
    strokeHint: 'One stroke: small top curve falling into a straight descent; dot on top.',
    example: { word: 'கார்', transliteration: 'kār', meaning: 'car' },
  },
  {
    id: 'mei-l',
    glyph: 'ல்',
    kind: 'mei',
    transliteration: 'l',
    tamilName: 'லகரம்',
    sound: 'l, as in lamp',
    strokeHint: 'A rounded bowl opening right, tail flicking up; dot above.',
    example: { word: 'பல்', transliteration: 'pal', meaning: 'tooth' },
  },
  {
    id: 'mei-v',
    glyph: 'வ்',
    kind: 'mei',
    transliteration: 'v',
    tamilName: 'வகரம்',
    sound: 'v / w, as in vine',
    strokeHint: 'Curve down and back up like a wide U leaning left; dot on top.',
    example: { word: 'வீடு', transliteration: 'vīṭu', meaning: 'house' },
  },
  {
    id: 'mei-zh',
    glyph: 'ழ்',
    kind: 'mei',
    transliteration: 'ḻ',
    tamilName: 'ழகரம்',
    sound: 'the retroflex "zh" -- unique to Tamil',
    strokeHint: 'Loop at the top, then the long curling tail that drops below the line.',
    example: { word: 'தமிழ்', transliteration: 'tamiḻ', meaning: 'Tamil' },
  },
  {
    id: 'mei-ll',
    glyph: 'ள்',
    kind: 'mei',
    transliteration: 'ḷ',
    tamilName: 'ளகரம்',
    sound: 'retroflex "l" -- tongue curled back',
    strokeHint: 'Like ல் with an extra upward hook on the right; dot above.',
    example: { word: 'பள்ளி', transliteration: 'paḷḷi', meaning: 'school' },
  },

  // --- ஆய்தம் + grantha ---
  {
    id: 'aaytham',
    glyph: 'ஃ',
    kind: 'aaytham',
    transliteration: 'ḵ',
    tamilName: 'ஆய்த எழுத்து',
    sound: 'a brief breath -- never stands alone in a word',
    strokeHint: 'Three dots in a triangle: two on the baseline, one above the gap.',
    example: { word: 'அஃது', transliteration: 'aḵtu', meaning: 'that' },
  },
  {
    id: 'grantha-j',
    glyph: 'ஜ',
    kind: 'grantha',
    transliteration: 'ja',
    tamilName: 'ஜகரம்',
    sound: 'j, as in jam -- used in loanwords',
    strokeHint: 'Three horizontal bars linked down the left edge.',
    example: { word: 'ஜன்னல்', transliteration: 'jannal', meaning: 'window' },
  },
  {
    id: 'grantha-sh',
    glyph: 'ஷ',
    kind: 'grantha',
    transliteration: 'ṣa',
    tamilName: 'ஷகரம்',
    sound: 'retroflex "sh"',
    strokeHint: 'ஈ-like double loop on the left with the upright stem on the right.',
    example: { word: 'கஷ்டம்', transliteration: 'kaṣṭam', meaning: 'difficulty' },
  },
  {
    id: 'grantha-s',
    glyph: 'ஸ',
    kind: 'grantha',
    transliteration: 'sa',
    tamilName: 'ஸகரம்',
    sound: 's, as in sun',
    strokeHint: 'Bowl on the left, then the vertical with a small flag at its top.',
    example: { word: 'பஸ்', transliteration: 'pas', meaning: 'bus' },
  },
  {
    id: 'grantha-h',
    glyph: 'ஹ',
    kind: 'grantha',
    transliteration: 'ha',
    tamilName: 'ஹகரம்',
    sound: 'h, as in hat',
    strokeHint: 'Left loop, crossbar, then the descending right stroke.',
    example: { word: 'ஹோட்டல்', transliteration: 'hōṭṭal', meaning: 'hotel' },
  },
];
