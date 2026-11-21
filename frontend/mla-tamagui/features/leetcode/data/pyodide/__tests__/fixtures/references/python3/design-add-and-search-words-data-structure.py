class WordDictionary:
    def __init__(self):
        self.root = {}

    def addWord(self, word: str) -> None:
        node = self.root
        for ch in word:
            node = node.setdefault(ch, {})
        node['$'] = True

    def search(self, word: str) -> bool:
        def walk(node, i):
            if i == len(word):
                return '$' in node
            ch = word[i]
            if ch == '.':
                return any(walk(child, i + 1) for key, child in node.items() if key != '$')
            return ch in node and walk(node[ch], i + 1)

        return walk(self.root, 0)
