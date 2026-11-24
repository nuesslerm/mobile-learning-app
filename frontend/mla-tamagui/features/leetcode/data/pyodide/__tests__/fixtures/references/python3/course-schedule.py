class Solution:
    def canFinish(self, numCourses: int, prerequisites: List[List[int]]) -> bool:
        from collections import deque
        adj=[[] for _ in range(numCourses)]; ind=[0]*numCourses
        for a,b in prerequisites: adj[b].append(a); ind[a]+=1
        q=deque(i for i in range(numCourses) if ind[i]==0); seen=0
        while q:
            u=q.popleft(); seen+=1
            for v in adj[u]:
                ind[v]-=1
                if ind[v]==0: q.append(v)
        return seen==numCourses
