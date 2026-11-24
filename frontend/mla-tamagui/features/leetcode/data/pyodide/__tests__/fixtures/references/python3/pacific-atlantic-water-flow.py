class Solution:
    def pacificAtlantic(self, heights: List[List[int]]) -> List[List[int]]:
        rows, cols = len(heights), len(heights[0])

        def reach(starts):
            seen, stack = set(starts), list(starts)
            while stack:
                r, c = stack.pop()
                for dr, dc in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    nr, nc = r + dr, c + dc
                    if 0 <= nr < rows and 0 <= nc < cols and (nr, nc) not in seen and heights[nr][nc] >= heights[r][c]:
                        seen.add((nr, nc))
                        stack.append((nr, nc))
            return seen

        pacific = reach([(r, 0) for r in range(rows)] + [(0, c) for c in range(cols)])
        atlantic = reach([(r, cols - 1) for r in range(rows)] + [(rows - 1, c) for c in range(cols)])
        return [[r, c] for r, c in sorted(pacific & atlantic)]
