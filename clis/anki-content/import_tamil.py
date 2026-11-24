"""Convert the approved Tamil Anki deck into the app's offline curriculum."""

import argparse
from collections import defaultdict
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import sqlite3
import unicodedata
from zipfile import ZipFile


SECTIONS = {
    "First steps": ["Greetings", "Expressions", "Numbers", "Colors", "Calendar", "Time"],
    "Home and food": ["Household", "Kitchen", "Food", "Groceries", "Fruits", "Vegetables", "Spices"],
    "People and health": ["Relations", "Friendship", "Romance", "Feelings", "Body", "Health"],
    "Getting around": ["Accommodation", "Driving", "Directions", "Shopping", "Travel", "Weather"],
    "Everyday life": ["Conversations", "General", "Work", "Study", "Sports", "Animals"],
}
TAG_NAMES = {"shoping": "Shopping", "romances": "Romance"}
SOURCE = "https://ankiweb.net/shared/info/2101840464"
NUMBER_LABEL = re.compile(r"^\d+\s*[-–:]\s*")
SOUND = re.compile(r"\[sound:([^\]]+)\]")
TAMIL_CORRECTIONS = {"mtw$JQ`*ON": "சாத்துக்குடி"}
TAMIL = re.compile(r"[\u0b80-\u0bff]")


class PlainText(HTMLParser):
    def __init__(self):
        super().__init__()
        self.parts = []

    def handle_data(self, data):
        self.parts.append(data)

    def handle_starttag(self, tag, attrs):
        if tag == "br":
            self.parts.append(" ")


def clean(field):
    parser = PlainText()
    parser.feed(SOUND.sub("", field))
    parser.close()
    return " ".join(unicodedata.normalize("NFC", "".join(parser.parts)).split())


def slug(text):
    return "-".join(text.lower().split())


def read_notes(archive, media):
    with sqlite3.connect(":memory:") as database:
        database.deserialize(archive.read("collection.anki2"))
        database.execute("PRAGMA query_only = ON")
        models = json.loads(database.execute("SELECT models FROM col").fetchone()[0])
        rows = database.execute("SELECT guid, mid, tags, flds FROM notes ORDER BY id").fetchall()

    notes = defaultdict(list)
    seen = set()
    for guid, model, tags, fields in rows:
        values = dict(zip((f["name"] for f in models[str(model)]["flds"]), fields.split("\x1f"), strict=True))
        english, tamil, roman = (clean(values[key]) for key in ("English", "Tamil", "Transiliration"))
        tamil = TAMIL_CORRECTIONS.get(guid, tamil)
        sounds = SOUND.findall(values["Sound"])
        if guid in seen or len(sounds) != 1 or not english or not TAMIL.search(tamil):
            raise ValueError(f"Invalid or duplicate Tamil note {guid}")
        seen.add(guid)
        tag, = tags.lower().split()
        topic = TAG_NAMES.get(tag, tag.title())
        if topic == "Numbers" and english.isdecimal():
            tamil = NUMBER_LABEL.sub("", tamil)
            roman = NUMBER_LABEL.sub("", roman)
        note_id = hashlib.sha256(guid.encode()).hexdigest()[:20]
        words = re.sub(r"\s*([,.;!?…]+)(?=\s|$)", "", tamil).split()
        if not words:
            raise ValueError(f"Missing answer words for {guid}")
        notes[topic].append({
            "id": note_id, "sourceGuid": guid, "english": english, "tamil": tamil,
            "transliteration": roman, "words": words, "asset": media[sounds[0]],
            "sourceAudio": sounds[0],
        })
    return notes


def curate(topic, notes):
    words_by_note = [[w for w in note["words"] if TAMIL.search(w)] for note in notes]
    vocabulary = list(dict.fromkeys(word for words in words_by_note for word in words))
    def difficulty_key(note):
        english = note["english"]
        if topic == "Numbers" and english.isdecimal():
            return (0, int(english))
        return (1, len(note["words"]))

    notes.sort(key=difficulty_key)
    task_size = 7
    task_count = (len(notes) + task_size - 1) // task_size
    base, extras = divmod(len(notes), task_count)
    start = 0
    for index in range(task_count):
        size = base + (index < extras)
        batch = notes[start:start + size]
        exercises = []
        for offset, note in enumerate(batch, start):
            words = note["words"]
            exercise = {**note, "kind": "translate"}
            if len(words) >= 3 and not note["english"].isdecimal() and offset % 3 == 1:
                exercise["kind"] = "fill"
                exercise["blank"] = max(
                    (i for i, word in enumerate(words) if TAMIL.search(word)),
                    key=lambda i: len(words[i]),
                )
            elif offset % 3 == 2:
                exercise["kind"] = "listen"

            position = offset % len(vocabulary)
            candidates = vocabulary[position:] + vocabulary[:position]
            excluded = {word.casefold() for word in words}
            exercise["distractors"] = [word for word in candidates if word.casefold() not in excluded][:2]
            exercises.append(exercise)
        longest = max(len(note["words"]) for note in batch)
        yield {
            "id": f"{slug(topic)}-{batch[0]['id']}", "number": index + 1,
            "title": f"{topic} · {batch[0]['english']}",
            "difficulty": "Easy" if longest <= 2 else "Medium" if longest <= 5 else "Hard",
            "exercises": exercises,
        }
        start += size


def generate(package, output, check):
    files = set()

    def emit(relative, contents):
        files.add(relative)
        target = output / relative
        if check:
            if not target.is_file() or target.read_bytes() != contents:
                raise ValueError(f"Out of date: {target}")
        else:
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(contents)

    with ZipFile(package) as archive:
        media = {name: key for key, name in json.loads(archive.read("media")).items()}
        notes = read_notes(archive, media)
        if set(notes) != {topic for topics in SECTIONS.values() for topic in topics}:
            raise ValueError("Deck topics differ from the curriculum map")
        if sum(map(len, notes.values())) != 1014:
            raise ValueError("Expected all 1,014 notes from the approved deck")

        sections = []
        audio_lines = []
        audio_size = 0
        for section, topics in SECTIONS.items():
            section_topics = []
            for topic in topics:
                tasks = list(curate(topic, notes[topic]))
                section_topics.append({"id": slug(topic), "title": topic, "tasks": tasks})
                for task in tasks:
                    for exercise in task["exercises"]:
                        contents = archive.read(exercise.pop("asset"))
                        relative = f"audio/{exercise['id']}.mp3"
                        audio_size += len(contents)
                        emit(relative, contents)
                        audio_lines.append(f"  '{exercise['id']}': () => require('./{relative}') as number,")
            sections.append({"id": slug(section), "title": section, "topics": section_topics})

    with package.open("rb") as original:
        checksum = hashlib.file_digest(original, "sha256").hexdigest()
    manifest = {
        "version": 1, "source": {"creator": "Charu Mamun", "url": SOURCE, "sha256": checksum,
        "permission": "Creator permission confirmed by the app owner on 2026-11-24."},
        "sections": sections,
    }
    emit("course.json", (json.dumps(manifest, ensure_ascii=False, indent=2) + "\n").encode())
    emit("audio.generated.ts", (
        "export const tamilAudio: Readonly<Record<string, () => number>> = {\n"
        + "\n".join(audio_lines) + "\n};\n"
    ).encode())
    actual = {file.relative_to(output).as_posix() for file in (output / "audio").glob("*.mp3")}
    extra = actual - files
    if extra:
        raise ValueError(f"Remove stale media files: {', '.join(sorted(extra))}")
    print(json.dumps({"notes": 1014, "topics": len(notes), "tamilAudio": len(audio_lines), "audioBytes": audio_size}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("deck", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    generate(args.deck, args.output, args.check)
