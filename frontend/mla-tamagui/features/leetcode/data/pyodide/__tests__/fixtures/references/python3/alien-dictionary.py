from collections import deque


class Solution:
    def alienOrder(self, words: List[str]) -> str:
        after = {ch: set() for word in words for ch in word}
        waiting = {ch: 0 for ch in after}
        for first, second in zip(words, words[1:]):
            if len(first) > len(second) and first.startswith(second):
                return ''
            for a, b in zip(first, second):
                if a != b:
                    if b not in after[a]:
                        after[a].add(b)
                        waiting[b] += 1
                    break
        ready = deque(ch for ch in waiting if waiting[ch] == 0)
        order = []
        while ready:
            ch = ready.popleft()
            order.append(ch)
            for nxt in after[ch]:
                waiting[nxt] -= 1
                if waiting[nxt] == 0:
                    ready.append(nxt)
        return ''.join(order) if len(order) == len(after) else ''
