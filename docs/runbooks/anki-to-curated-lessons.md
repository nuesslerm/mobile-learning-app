# Convert an Anki deck into curated offline lessons

The worked example is [Learn Tamil Quickly English to Tamil by Charu Mamun](https://ankiweb.net/shared/info/2101840464). The app owner confirmed permission to use the deck and its recordings on November 24, 2026. The source package SHA-256 is `c455e8911549c1d94ba87ec11bc20bda7f3599c54ea166a31c96bb1d23ff214e`.

## Inspect the source

1. Record the creator, source URL, package checksum, what the supplied rights cover, and the credit to show in the app.
2. Inspect the archive format and collection schema. Tamil uses `collection.anki2`, a SQLite database, and a JSON `media` manifest. For a newer Anki format, export a legacy-compatible package from Anki or add a separately tested reader. Do not assume numeric ZIP member names describe the files.
3. Read `col.models` for field names, then join `notes.mid` to the note type. Preserve each `notes.guid`. Check field coverage and topic tags, and inspect `cards.did` and the deck hierarchy if the source has no useful tags.
4. Resolve each sound reference through the media manifest. The Tamil deck has an English `[sound:…]` marker in `English` and a Tamil one in `Sound`. The importer uses only `Sound` for assets. Normalize plain text, preserve the original register, and keep source corrections keyed by GUID.

## Curate and generate

1. Map every source topic to an app section. Establish a progression from vocabulary and fixed phrases to short exchanges and longer sentences. Keep tasks short, with a worked example followed by scored prompts.
2. Make each note the primary target of a scored prompt. Choose tile or typed translation when the gloss is unambiguous, listening when its recording identifies the answer, and sentence completion when the blank and English clue identify the target. Use topic-relevant distractors and remove duplicates and plausible alternative answers.
3. Review every note’s target script, translation, transliteration, recording alignment, grading, and distractors. Record any editorial change by source GUID. In the Tamil deck, the Sweet lime note contains `ac` in the Tamil field. The importer supplies `சாத்துக்குடி` for that GUID. The importer also strips number labels from Tamil answers and orders cardinal numbers numerically.
4. From the repository root, regenerate the approved Tamil course. Keep the source package outside the tracked tree.

   ```sh
   wf content import-tamil '/path/to/Learn_Tamil_Quickly_English_to_Tamil.apkg'
   wf content import-tamil '/path/to/Learn_Tamil_Quickly_English_to_Tamil.apkg' --check
   ```

   A successful check reports 1,014 source notes, 31 topics, 1,014 Tamil audio clips, and total audio bytes. It compares the regenerated manifest and each clip byte for byte and rejects stale files. The importer embeds the 128 smallest recordings as base64 alongside the regular assets to stay within the EAS Update asset count. Exercise IDs and MP3 bytes are identical in both representations.

## Verify the experience

- Run the Tamil curriculum and progress tests. Compare source GUIDs with the scored prompts, confirm every target has audio, and verify English audio stays out of the asset map.
- Reimport a revised deck and check that unchanged GUIDs retain their exercise IDs and that saved attempts still resolve. Review any moved notes before changing the order or identity of existing published tasks.
- On a fresh simulator, open the course offline and exercise every mechanic, both playback speeds, task resume, and progress after relaunch. Capture screenshots and recordings, then review the bundle size and verify the web export before publishing.
