var coinChange = function(coins, amount) {
  const fewest = new Array(amount + 1).fill(Infinity);
  fewest[0] = 0;
  for (let total = 1; total <= amount; total += 1) {
    for (const coin of coins) {
      if (coin <= total) fewest[total] = Math.min(fewest[total], fewest[total - coin] + 1);
    }
  }
  return fewest[amount] === Infinity ? -1 : fewest[amount];
};
