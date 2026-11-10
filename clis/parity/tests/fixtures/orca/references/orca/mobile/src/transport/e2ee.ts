export function handshake(key: Uint8Array) {
  return box(key)
}
