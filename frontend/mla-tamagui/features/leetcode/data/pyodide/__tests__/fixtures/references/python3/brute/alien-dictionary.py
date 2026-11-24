from itertools import permutations


class Solution:
    def alienOrder(self, words: List[str]) -> str:
        letters = sorted({ch for word in words for ch in word})
        if len(letters) > 6:
            raise ValueError('the oracle enumerates at most 6 letters, got %d' % len(letters))
        valid = []
        for order in permutations(letters):
            rank = {ch: i for i, ch in enumerate(order)}
            keys = [[rank[ch] for ch in word] for word in words]
            if all(a <= b for a, b in zip(keys, keys[1:])):
                valid.append(''.join(order))
        if len(valid) > 1:
            raise ValueError('more than one order fits: %r' % valid)
        return valid[0] if valid else ''
