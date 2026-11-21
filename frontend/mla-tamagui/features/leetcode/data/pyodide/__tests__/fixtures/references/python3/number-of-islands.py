class Solution:
    def numIslands(self, grid: List[List[str]]) -> int:
        g=[r[:] for r in grid]; n=0
        for i in range(len(g)):
            for j in range(len(g[0])):
                if g[i][j]=='1':
                    n+=1; st=[(i,j)]; g[i][j]='0'
                    while st:
                        a,b=st.pop()
                        for x,y in((a+1,b),(a-1,b),(a,b+1),(a,b-1)):
                            if 0<=x<len(g) and 0<=y<len(g[0]) and g[x][y]=='1':
                                g[x][y]='0'; st.append((x,y))
        return n
