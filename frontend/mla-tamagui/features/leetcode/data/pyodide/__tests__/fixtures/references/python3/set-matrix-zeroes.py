class Solution:
    def setZeroes(self, matrix: List[List[int]]) -> None:
        rows = {i for i, r in enumerate(matrix) if 0 in r}
        cols = {j for r in matrix for j, v in enumerate(r) if v == 0}
        for i in range(len(matrix)):
            for j in range(len(matrix[0])):
                if i in rows or j in cols:
                    matrix[i][j] = 0
