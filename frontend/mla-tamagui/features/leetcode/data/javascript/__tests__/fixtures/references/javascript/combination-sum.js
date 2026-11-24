var combinationSum = function(candidates, target) {
  const combinations = [];
  const path = [];
  const walk = (start, left) => {
    if (left === 0) {
      combinations.push([...path]);
      return;
    }
    for (let i = start; i < candidates.length; i += 1) {
      if (candidates[i] > left) continue;
      path.push(candidates[i]);
      walk(i, left - candidates[i]);
      path.pop();
    }
  };
  walk(0, target);
  return combinations;
};
