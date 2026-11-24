import { File, Paths } from 'expo-file-system';

export function packedAudioUri(id: string, base64: string): string {
  const file = new File(Paths.cache, `tamil-${id}.mp3`);
  file.create({ overwrite: true });
  file.writeSync(base64, { encoding: 'base64' });
  return file.uri;
}
