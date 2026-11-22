var exist = function(board, word) {
  const rows = board.length;
  const cols = board[0].length;
  const search = (r, c, at) => {
    if (at === word.length) return true;
    if (r < 0 || r >= rows || c < 0 || c >= cols || board[r][c] !== word[at]) return false;
    const letter = board[r][c];
    board[r][c] = '#';
    const found = search(r + 1, c, at + 1) || search(r - 1, c, at + 1) || search(r, c + 1, at + 1) || search(r, c - 1, at + 1);
    board[r][c] = letter;
    return found;
  };
  for (let r = 0; r < rows; r += 1) {
    for (let c = 0; c < cols; c += 1) {
      if (search(r, c, 0)) return true;
    }
  }
  return false;
};
