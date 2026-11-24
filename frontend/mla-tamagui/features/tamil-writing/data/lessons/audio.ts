import { Logger } from '@lib/logging';
import { createAudioPlayer, setAudioModeAsync, type AudioPlayer } from 'expo-audio';
import { Platform } from 'react-native';

import type { TamilAudioPort } from '../../domain/lessons/ports';
import { tamilAudio } from './audio.generated';
import packedAudio from './audio.packed.json';
import { packedAudioUri } from './packedAudioUri';

const packedTamilAudio: Readonly<Partial<Record<string, string>>> = packedAudio;

export function createExpoTamilAudio(): TamilAudioPort {
  let current: AudioPlayer | undefined;
  let unsubscribe: (() => void) | undefined;
  let playback = 0;
  function stop() {
    playback += 1;
    unsubscribe?.();
    unsubscribe = undefined;
    current?.remove();
    current = undefined;
  }
  return {
    async play(id, slow) {
      stop();
      const request = playback;
      if (Platform.OS !== 'web') await setAudioModeAsync({ playsInSilentMode: true });
      if (request !== playback) return;
      const packed = packedTamilAudio[id];
      const source = packed === undefined ? tamilAudio[id]?.() : packedAudioUri(id, packed);
      if (source === undefined) throw new Error('Tamil recording is unavailable');
      const player = createAudioPlayer(source, { updateInterval: 100 });
      current = player;
      player.shouldCorrectPitch = true;
      player.setPlaybackRate(slow ? 0.75 : 1);
      const subscription = player.addListener('playbackStatusUpdate', (status) => {
        if (!status.playing || status.currentTime <= 0) return;
        Logger.info('tamil.audio.started', { exerciseId: id, rate: status.playbackRate });
        subscription.remove();
        unsubscribe = undefined;
      });
      unsubscribe = () => subscription.remove();
      player.play();
    },
    stop,
  };
}
