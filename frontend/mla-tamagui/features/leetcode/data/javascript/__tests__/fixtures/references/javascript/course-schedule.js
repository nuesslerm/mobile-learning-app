var canFinish = function(numCourses, prerequisites) {
  const unlocks = Array.from({ length: numCourses }, () => []);
  const blockers = new Array(numCourses).fill(0);
  for (const [course, prerequisite] of prerequisites) {
    unlocks[prerequisite].push(course);
    blockers[course] += 1;
  }
  const ready = [];
  for (let course = 0; course < numCourses; course += 1) {
    if (blockers[course] === 0) ready.push(course);
  }
  let taken = 0;
  while (ready.length > 0) {
    const course = ready.pop();
    taken += 1;
    for (const next of unlocks[course]) {
      blockers[next] -= 1;
      if (blockers[next] === 0) ready.push(next);
    }
  }
  return taken === numCourses;
};
