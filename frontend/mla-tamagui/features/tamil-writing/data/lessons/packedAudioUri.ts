export function packedAudioUri(_id: string, base64: string): string {
  return `data:audio/mpeg;base64,${base64}`;
}
