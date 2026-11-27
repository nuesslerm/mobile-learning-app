import { Logger } from '@lib/logging';
import { createAudioPlayer, setAudioModeAsync, type AudioPlayer } from 'expo-audio';
import { AppState, Platform } from 'react-native';

import type { TamilAudioPort } from '../../domain/lessons/ports';
import { audioCues, sectionTracks } from './audio.generated';

export function createExpoTamilAudio(): TamilAudioPort {
  let current: AudioPlayer | undefined;
  let cleanup: (() => void) | undefined;
  let playback = 0;
  function stop() {
    playback += 1;
    cleanup?.();
    cleanup = undefined;
    current?.remove();
    current = undefined;
  }
  return {
    async play(id, slow, onStarted) {
      stop();
      const request = playback;
      const cue = audioCues[id];
      if (!cue) throw new Error('Tamil recording is unavailable');
      const [section, startMs, endMs] = cue;
      if (Platform.OS !== 'web') await setAudioModeAsync({ playsInSilentMode: true });
      if (request !== playback) return;
      const player = createAudioPlayer(sectionTracks[section](), { updateInterval: 50 });
      current = player;
      let started = false;
      let timer: ReturnType<typeof setTimeout> | undefined;
      let cancelLoad: (() => void) | undefined;
      const rate = slow ? 0.75 : 1;
      const end = endMs / 1000;
      const checkEnd = () => {
        if (request !== playback) return;
        const remaining = end - player.currentTime;
        if (remaining <= 0) stop();
        else timer = setTimeout(checkEnd, Math.max(20, remaining * 1000 / rate));
      };
      const listener = player.addListener('playbackStatusUpdate', (status) => {
        if (request !== playback || !status.playing) return;
        if (status.currentTime >= end) { stop(); return; }
        if (started || status.currentTime <= startMs / 1000) return;
        started = true;
        checkEnd();
        Logger.info('tamil.audio.started', { exerciseId: id, rate });
        onStarted?.();
      });
      const lifecycle = AppState.addEventListener('change', state => {
        if (state !== 'active' && request === playback) stop();
      });
      cleanup = () => {
        cancelLoad?.();
        listener.remove();
        lifecycle.remove();
        clearTimeout(timer);
      };
      try {
        if (Platform.OS !== 'web' && !player.isLoaded) {
          await new Promise<void>((resolve, reject) => {
            const subscription = player.addListener('playbackStatusUpdate', status => {
              if (status.error) reject(new Error(status.error));
              else if (status.isLoaded) resolve();
            });
            cancelLoad = () => { subscription.remove(); resolve(); };
            if (player.isLoaded) resolve();
          });
          cancelLoad?.();
          cancelLoad = undefined;
        }
        if (request !== playback) return;
        await player.seekTo(startMs / 1000, 0, 0);
        if (request !== playback) return;
        player.shouldCorrectPitch = true;
        player.setPlaybackRate(rate);
        player.play();
      } catch (cause) {
        if (request !== playback) return;
        stop();
        throw cause;
      }
    },
    stop,
  };
}
