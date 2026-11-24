class Solution:
    def exist(self, board: List[List[str]], word: str) -> bool:
        rows, cols = len(board), len(board[0])

        def walk(r, c, i):
            if i == len(word):
                return True
            if not (0 <= r < rows and 0 <= c < cols) or board[r][c] != word[i]:
                return False
            ch, board[r][c] = board[r][c], '#'
            found = any(walk(r + dr, c + dc, i + 1) for dr, dc in ((1, 0), (-1, 0), (0, 1), (0, -1)))
            board[r][c] = ch
            return found

        return any(walk(r, c, 0) for r in range(rows) for c in range(cols))
