# Package offline lesson audio by section

The Tamil course ships five MP3s for 1,014 exercises. Metro includes every statically referenced recording in an Expo update, even when the initial route is another feature. Grouping short recordings by curriculum section keeps offline playback and makes the asset count grow with sections. Editing one section can require downloading that whole section again.

## Add a recording

1. Add the authorized recording to the source deck. Set the note’s Tamil **Sound** field and its topic tag. Preserve the note GUID when revising a phrase so saved progress still resolves. Keep the original deck outside the repository.
2. For a new topic or section, update the section map in the importer. When adding or removing notes, update the approved deck count, the curriculum coverage assertion, and the source checksum in the runbook. Review the generated task IDs and order if the deck contents changed.
3. Install FFmpeg with the libmp3lame encoder. From the repository root, run the import and then the independent regeneration check with the same FFmpeg build.

   ```sh
   wf content import-tamil /path/to/approved-deck.apkg
   wf content import-tamil /path/to/approved-deck.apkg --check
   ```

4. Review and commit the generated course, section MP3s, and cue index together. A renamed or removed section can leave an obsolete track. The importer reports it so you can review and remove it. The app imports only the section tracks.

## Keep cue times aligned

The importer decodes each recording to 44.1 kHz, mono, signed 16-bit PCM. It counts the samples written to the section track and emits each exercise’s start and exclusive end in milliseconds. Then it writes 500 ms of silence before the next phrase and encodes the section once. Summing MP3 file durations can accumulate encoder delay across hundreds of recordings.

Use the same cue for normal and slow playback. The timestamps describe positions in the media; the player changes its playback rate. Stop when the media position reaches the end. The silence gives playback callbacks room to arrive before the next phrase. Cancelling an exercise must also cancel any outstanding load, seek, and end timer.

## Verify a change

Run the Tamil curriculum test to check that every exercise appears once, belongs to the correct section, and has a positive cue duration followed by the expected gap. Play the first, middle, and last exercise in a section, at both speeds. Switch exercises during playback and check that the previous one stops. After an Expo export, count the audio entries in its actual manifest:

```sh
jq '[.fileMetadata.ios.assets[] | select(.ext == "mp3")] | length' frontend/mla-tamagui/dist/metadata.json
```

With the current Tamil course and no other MP3 sources, the expected count is five. See the [Anki import runbook](../../runbooks/anki-to-curated-lessons.md) for the approved source and curriculum checks.
