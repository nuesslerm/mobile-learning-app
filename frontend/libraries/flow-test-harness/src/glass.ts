// The expo-glass-effect stub answers both availability checks from this switch; the setup file turns it off after each test.
const g = globalThis as { __flowGlass?: { available: boolean } };

export function setLiquidGlass(available: boolean): void {
  g.__flowGlass = { available };
}

export function resetLiquidGlass(): void {
  g.__flowGlass = { available: false };
}
