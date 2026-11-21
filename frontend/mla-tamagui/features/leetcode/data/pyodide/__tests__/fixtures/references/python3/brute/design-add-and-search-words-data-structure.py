import re


class WordDictionary:
    def __init__(self):
        self.words = []

    def addWord(self, word: str) -> None:
        self.words.append(word)

    def search(self, word: str) -> bool:
        pattern = re.compile(word)
        return any(pattern.fullmatch(w) for w in self.words)
