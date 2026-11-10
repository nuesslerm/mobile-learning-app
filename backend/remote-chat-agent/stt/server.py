"""Speech-to-text sidecar for remote-chat-agent.

faster-whisper (CTranslate2, int8) behind one HTTP route with the same
multipart contract as the agent's /api/voice-transcribe -- an `audio` file
part -- so the agent, the harness scorer and a curl all speak to it the same
way. Measured on the 10-sentence voice fixture set: 4.2x faster than the
whisper.cpp build the VM ran, at equal or better accuracy
(docs/audits/2026-09-14-voice-edit-latency-data.md, section 8).

Configuration is environment only:
  STT_MODEL           tiny.en | base.en | small.en ... (default tiny.en)
  STT_COMPUTE         int8 | int8_float32 | float32 (default int8)
  STT_THREADS         CPU threads (default: all)
  STT_BEAM            beam size (default 1 -- greedy; beam 5 was slower and no more accurate)
  STT_INITIAL_PROMPT  domain vocabulary whisper is primed with (default: code-editing terms)
  PORT                listen port (default 8790)
"""
from __future__ import annotations

import json
import os
import tempfile
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from email.parser import BytesParser
from email.policy import default as email_policy

from faster_whisper import WhisperModel

MODEL_NAME = os.environ.get("STT_MODEL", "tiny.en")
COMPUTE = os.environ.get("STT_COMPUTE", "int8")
THREADS = int(os.environ.get("STT_THREADS") or os.cpu_count() or 1)
BEAM = int(os.environ.get("STT_BEAM") or 1)
DEFAULT_PROMPT = (
    "Spoken instructions for editing Python code: rename the variable, declare a left pointer "
    "and a right pointer, while loop, pass body, return true, return false, alphanumeric characters, "
    "lowercase, docstring, try catch, delete the print statement, loop condition, less than n, index."
)
# Compose passes the variable through as "" when unset; empty means "use the default", not "no prompt".
INITIAL_PROMPT = os.environ.get("STT_INITIAL_PROMPT") or DEFAULT_PROMPT
PORT = int(os.environ.get("PORT") or 8790)

print(json.dumps({"event": "stt.loading", "model": MODEL_NAME, "compute": COMPUTE, "threads": THREADS}), flush=True)
_t0 = time.time()
MODEL = WhisperModel(MODEL_NAME, device="cpu", compute_type=COMPUTE, cpu_threads=THREADS)
print(json.dumps({"event": "stt.ready", "loadMs": int((time.time() - _t0) * 1000)}), flush=True)


def transcribe_file(path: str, prompt: str | None) -> str:
    segments, _info = MODEL.transcribe(
        path,
        language="en",
        beam_size=BEAM,
        vad_filter=False,
        initial_prompt=prompt if prompt else None,
        condition_on_previous_text=False,
    )
    return " ".join(s.text.strip() for s in segments).strip()


def parse_multipart(handler: BaseHTTPRequestHandler) -> tuple[bytes | None, str, dict[str, str]]:
    length = int(handler.headers.get("Content-Length") or 0)
    body = handler.rfile.read(length)
    content_type = handler.headers.get("Content-Type", "")
    if not content_type.startswith("multipart/form-data"):
        return body, "audio.bin", {}
    message = BytesParser(policy=email_policy).parsebytes(
        b"Content-Type: " + content_type.encode() + b"\r\nMIME-Version: 1.0\r\n\r\n" + body
    )
    audio, filename, fields = None, "audio.bin", {}
    for part in message.iter_parts():
        name = part.get_param("name", header="content-disposition")
        if name == "audio":
            audio = part.get_payload(decode=True)
            filename = part.get_filename() or filename
        elif name:
            fields[name] = part.get_content().strip() if isinstance(part.get_content(), str) else ""
    return audio, filename, fields


class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):  # one JSON line per request instead of the default access log
        pass

    def _json(self, status: int, payload: dict) -> None:
        data = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path.startswith("/health"):
            self._json(200, {"status": "ok", "model": MODEL_NAME, "compute": COMPUTE, "threads": THREADS, "beam": BEAM})
        else:
            self._json(404, {"error": "not_found"})

    def do_POST(self):
        if not self.path.startswith("/transcribe"):
            self._json(404, {"error": "not_found"})
            return
        audio, filename, fields = parse_multipart(self)
        if not audio:
            self._json(400, {"error": "No audio file uploaded"})
            return
        # `prompt` overrides the configured vocabulary; `prompt=` (empty) disables it -- the scorer's A/B switch.
        prompt = fields.get("prompt", INITIAL_PROMPT) if "prompt" in fields else INITIAL_PROMPT
        suffix = os.path.splitext(filename)[1] or ".bin"
        started = time.time()
        with tempfile.NamedTemporaryFile(suffix=suffix, delete=True) as tmp:
            tmp.write(audio)
            tmp.flush()
            try:
                transcript = transcribe_file(tmp.name, prompt)
            except Exception as err:  # noqa: BLE001 -- surface every decode failure as a 500 with its text
                self._json(500, {"error": "stt_failed", "message": str(err)[:500]})
                return
        ms = int((time.time() - started) * 1000)
        print(json.dumps({"event": "stt.transcribed", "ms": ms, "bytes": len(audio), "chars": len(transcript), "model": MODEL_NAME}), flush=True)
        self._json(200, {"transcript": transcript, "ms": ms, "model": f"faster-whisper:{MODEL_NAME}:{COMPUTE}", "threads": THREADS})


if __name__ == "__main__":
    ThreadingHTTPServer(("0.0.0.0", PORT), Handler).serve_forever()
