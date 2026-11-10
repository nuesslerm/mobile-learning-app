// An Android deep link loses raw newlines in a query value (16 Oct: a seeded editor showed its code on one line), so the e2e
// tests send a dev seed as base64url in `seed64`; `seed` stays for links typed by hand.
// Only a dev bundle or the device-lab export takes a seed: a production build ignores the parameter.
export function devSeed({ seed, seed64 }: { seed?: string; seed64?: string }): string | undefined {
  if (!__DEV__ && process.env.EXPO_PUBLIC_DEVICE_LAB !== '1') return undefined;
  if (!seed64) return seed;
  const b64 = seed64.replace(/-/g, '+').replace(/_/g, '/');
  return atob(b64 + '='.repeat((4 - (b64.length % 4)) % 4));
}
