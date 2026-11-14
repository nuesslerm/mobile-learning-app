export type WritingVerdict = {
  ok: boolean;
  /** 0-100, the model's own confidence that this reads as the letter. */
  score: number;
  summary: string;
  fix?: string;
};

export type WritingCheckRequest = {
  characterId: string;
  glyph: string;
  transliteration: string;
  /** The ink alone, black on white -- never the template. */
  svg: string;
};

export class WritingCheckError extends Error {
  constructor(
    readonly status: number,
    message: string
  ) {
    super(message);
    this.name = 'WritingCheckError';
  }
}

/** Marks what was written: a freehand letter is a recognition problem, answered by something that can read Tamil. */
export type WritingCheckPort = {
  check(request: WritingCheckRequest): Promise<WritingVerdict>;
};

export type TamilWritingPorts = {
  writingCheck: WritingCheckPort;
};
