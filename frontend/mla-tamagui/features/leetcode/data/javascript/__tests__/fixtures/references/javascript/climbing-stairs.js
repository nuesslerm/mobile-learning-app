var climbStairs = function(n) {
  let previous = 1;
  let current = 1;
  for (let step = 2; step <= n; step += 1) {
    [previous, current] = [current, previous + current];
  }
  return current;
};
