/**
 * Field-name drift the models show on otherwise correct replies, folded back
 * to the schema before validation. Models settle on `newText` (the
 * replaceLines field) for insertAfterLine about as often as on `text`; both
 * name the same block. Pure: a conforming document comes back equal.
 */
export function normalizeVoiceEditResponse(raw: unknown): unknown {
  if (typeof raw !== 'object' || raw === null) return raw;
  const envelope = raw as { alternatives?: unknown };
  if (!Array.isArray(envelope.alternatives)) return raw;
  return {
    ...envelope,
    alternatives: envelope.alternatives.map((alternative) => {
      if (typeof alternative !== 'object' || alternative === null)
        return alternative;
      const alt = alternative as { commands?: unknown };
      if (!Array.isArray(alt.commands)) return alternative;
      return { ...alt, commands: alt.commands.map(normalizeCommand) };
    }),
  };
}

function normalizeCommand(raw: unknown): unknown {
  if (typeof raw !== 'object' || raw === null) return raw;
  const cmd = raw as { type?: unknown; text?: unknown; newText?: unknown };
  if (
    cmd.type === 'insertAfterLine' &&
    cmd.text === undefined &&
    typeof cmd.newText === 'string'
  ) {
    const { newText, ...rest } = cmd;
    return { ...rest, text: newText };
  }
  if (
    cmd.type === 'replaceLines' &&
    cmd.newText === undefined &&
    typeof cmd.text === 'string'
  ) {
    const { text, ...rest } = cmd;
    return { ...rest, newText: text };
  }
  return raw;
}
