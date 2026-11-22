var findWords = function(board, words) {
  const root = { children: new Map(), word: null };
  for (const word of words) {
    let node = root;
    for (const ch of word) {
      if (!node.children.has(ch)) node.children.set(ch, { children: new Map(), word: null });
      node = node.children.get(ch);
    }
    node.word = word;
  }
  const rows = board.length;
  const cols = board[0].length;
  const found = [];
  const visit = (r, c, parent) => {
    const letter = board[r][c];
    const node = parent.children.get(letter);
    if (node === undefined) return;
    if (node.word !== null) {
      found.push(node.word);
      node.word = null;
    }
    board[r][c] = '#';
    for (const [dr, dc] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
      const nr = r + dr;
      const nc = c + dc;
      if (nr >= 0 && nr < rows && nc >= 0 && nc < cols && board[nr][nc] !== '#') visit(nr, nc, node);
    }
    board[r][c] = letter;
  };
  for (let r = 0; r < rows; r += 1) {
    for (let c = 0; c < cols; c += 1) visit(r, c, root);
  }
  return found;
};
