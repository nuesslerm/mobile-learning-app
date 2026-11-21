/**
 * Base64 encoding for the ORDS Basic Auth header.
 *
 * Deliberately does NOT use Node's `Buffer` (not universally available in
 * the RN/Hermes JS engine this code also runs in) and does NOT assume a
 * global `btoa` (only guaranteed on web / React Native >= 0.72's Hermes — an
 * older engine or a vitest/node test run may not have it). A small
 * hand-rolled implementation works identically everywhere this package's
 * code executes.
 */

const BASE64_ALPHABET =
  'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';

export function encodeBase64(input: string): string {
  let output = '';
  let i = 0;
  while (i < input.length) {
    const byte1 = input.charCodeAt(i++) & 0xff;
    const hasByte2 = i < input.length;
    const byte2 = hasByte2 ? input.charCodeAt(i++) & 0xff : 0;
    const hasByte3 = i < input.length;
    const byte3 = hasByte3 ? input.charCodeAt(i++) & 0xff : 0;

    const triplet = (byte1 << 16) | (byte2 << 8) | byte3;

    output += BASE64_ALPHABET[(triplet >> 18) & 0x3f];
    output += BASE64_ALPHABET[(triplet >> 12) & 0x3f];
    output += hasByte2 ? BASE64_ALPHABET[(triplet >> 6) & 0x3f] : '=';
    output += hasByte3 ? BASE64_ALPHABET[triplet & 0x3f] : '=';
  }
  return output;
}
