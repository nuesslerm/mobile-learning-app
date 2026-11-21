class Solution:
    def combinationSum(self, candidates: List[int], target: int) -> List[List[int]]:
        out = []

        def walk(start, left, path):
            if left == 0:
                out.append(path[:])
                return
            for i in range(start, len(candidates)):
                if candidates[i] <= left:
                    path.append(candidates[i])
                    walk(i, left - candidates[i], path)
                    path.pop()

        walk(0, target, [])
        return out
