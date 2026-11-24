import { Logger } from '@lib/logging';
import { useAudioPlayer, useAudioPlayerStatus } from 'expo-audio';
import { useCallback } from 'react';

export interface VoicePlaybackResult {
  playing: boolean;
  currentTime: number;
  duration: number;
  toggle: () => void;
}

/**
 * Playback of the just-recorded clip in the review sheet. Owns the expo-audio
 * player so the UI layer never imports the native audio module directly.
 */
export function useVoicePlayback(recordingUri: string | null): VoicePlaybackResult {
  const player = useAudioPlayer(recordingUri);
  const status = useAudioPlayerStatus(player);

  const toggle = useCallback(() => {
    if (status.playing) {
      Logger.info('voice_edit.review.pause', {});
      player.pause();
    } else {
      Logger.info('voice_edit.review.play', {});
      // Play again from the start once a full playthrough has finished --
      // otherwise a second tap after the clip ends does nothing, since the
      // player is technically still "loaded" at its end position.
      if (status.currentTime >= status.duration && status.duration > 0) {
        void player.seekTo(0);
      }
      player.play();
    }
  }, [player, status.playing, status.currentTime, status.duration]);

  return {
    playing: status.playing,
    currentTime: status.currentTime,
    duration: status.duration,
    toggle,
  };
}