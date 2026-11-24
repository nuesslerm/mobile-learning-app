from collections import Counter


class Solution:
    def topKFrequent(self, nums: List[int], k: int) -> List[int]:
        return [n for n, _ in Counter(nums).most_common(k)]
