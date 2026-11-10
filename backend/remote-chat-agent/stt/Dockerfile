# Speech-to-text sidecar: faster-whisper (CTranslate2 int8) on CPU. Models are
# baked in at build time so a redeploy never waits on Hugging Face; STT_MODEL
# picks one at runtime. Runs in the remote-chat-agent compose project on the
# same tailscale netns as the agent, which reaches it at localhost:8790.
FROM python:3.11-slim
ENV PYTHONUNBUFFERED=1 HF_HUB_OFFLINE=0
WORKDIR /app
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt
ARG STT_MODELS="tiny.en base.en"
ENV STT_MODELS=${STT_MODELS}
RUN python - <<'PY'
import os
from faster_whisper import WhisperModel
for name in os.environ.get("STT_MODELS", "tiny.en base.en").split():
    WhisperModel(name, device="cpu", compute_type="int8")
    print("cached", name)
PY
ENV HF_HUB_OFFLINE=1
COPY server.py .
EXPOSE 8790
CMD ["python", "server.py"]
