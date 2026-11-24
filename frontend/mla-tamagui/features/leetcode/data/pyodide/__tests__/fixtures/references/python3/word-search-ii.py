class Solution:
    def findWords(self, board: List[List[str]], words: List[str]) -> List[str]:
        trie = {}
        for word in words:
            node = trie
            for ch in word:
                node = node.setdefault(ch, {})
            node['$'] = word
        rows, cols = len(board), len(board[0])
        found = []

        def walk(r, c, parent):
            ch = board[r][c]
            node = parent.get(ch)
            if node is None:
                return
            if '$' in node:
                found.append(node.pop('$'))
            board[r][c] = '#'
            for dr, dc in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                nr, nc = r + dr, c + dc
                if 0 <= nr < rows and 0 <= nc < cols and board[nr][nc] != '#':
                    walk(nr, nc, node)
            board[r][c] = ch

        for r in range(rows):
            for c in range(cols):
                walk(r, c, trie)
        return found
