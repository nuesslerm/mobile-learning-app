import type { EditAlternative, EditCommand } from '../contracts/schema';

const BLOCK_HEADER = /^(\s*)(?:if|elif|else|for|while|try|except|finally|with|def|class)\b.*:\s*$/;

function indentOf(line: string): number {
  return line.length - line.trimStart().length;
}

/**
 * A spoken "loop over the numbers" or "add an if" opens a block and says nothing of its body. A model that stops at the
 * header leaves Python that does not parse, and the speaker's next Run Local answers CompileError. So an insert or
 * replacement that ends on a block header with nothing deeper after it gets a `pass` body one level in, as every
 * runbook step that opens a block asks for by hand ("with a pass body").
 */
export function completeBlockBodies(alternative: EditAlternative, code: string, language: string): EditAlternative {
  if (language.toLowerCase() !== 'python') return alternative;
  const lines = code.split('\n');
  const commands = alternative.commands.map((command): EditCommand => {
    if (command.type !== 'insertAfterLine' && command.type !== 'replaceLines') return command;
    const body = command.type === 'insertAfterLine' ? command.text : command.newText;
    const added = body.split('\n');
    const last = added[added.length - 1] ?? '';
    const header = BLOCK_HEADER.exec(last);
    if (!header) return command;
    const next = lines[command.type === 'insertAfterLine' ? command.line : command.endLine];
    if (next !== undefined && next.trim() !== '' && indentOf(next) > header[1]!.length) return command;
    const completed = `${body}\n${header[1]}    pass`;
    return command.type === 'insertAfterLine' ? { ...command, text: completed } : { ...command, newText: completed };
  });
  return { ...alternative, commands };
}
