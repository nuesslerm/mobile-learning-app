// The PTY can redraw the current input line, move the cursor or clear a previous line.
export function terminalText(output: string): string {
  const rows: string[][] = [[]];
  let row = 0;
  let col = 0;
  const line = () => (rows[row] ??= []);
  const chars = Array.from(output);
  for (let i = 0; i < chars.length; i++) {
    const ch = chars[i];
    if (ch === '\x1b') {
      const type = chars[++i];
      if (type === ']') {
        while (++i < chars.length && chars[i] !== '\x07') {
          if (chars[i] === '\x1b' && chars[i + 1] === '\\') {
            i++;
            break;
          }
        }
      } else if (type === '[') {
        let args = '';
        while (++i < chars.length && !/^[A-Za-z@~]$/.test(chars[i]))
          args += chars[i];
        const [a, b] = args.split(';').map(Number);
        const step = Math.min(4096, Math.max(1, a || 1));
        switch (chars[i]) {
          case 'K':
            if (a === 2) rows[row] = [];
            else if (a === 1) line().fill(' ', 0, col + 1);
            else line().splice(col);
            break;
          case 'J':
            if (a === 2 || a === 3) {
              rows.length = 1;
              rows[0] = [];
              row = col = 0;
            }
            break;
          case 'A':
            row = Math.max(0, row - step);
            break;
          case 'B':
            row = Math.min(10000, row + step);
            break;
          case 'C':
            col = Math.min(4096, col + step);
            break;
          case 'D':
            col = Math.max(0, col - step);
            break;
          case 'G':
            col = step - 1;
            break;
          case 'H':
          case 'f':
            row = step - 1;
            col = Math.min(4096, Math.max(0, (b || 1) - 1));
            break;
        }
      }
    } else if (ch === '\r') col = 0;
    else if (ch === '\n') {
      row++;
      col = 0;
      line();
    } else if (ch === '\b') col = Math.max(0, col - 1);
    else if (ch === '\t') col = Math.min(4096, 8 * (1 + Math.floor(col / 8)));
    else if (ch >= ' ') {
      const current = line();
      while (current.length < col) current.push(' ');
      current[col++] = ch;
    }
  }
  return rows.map((r) => r?.join('') ?? '').join('\n');
}
