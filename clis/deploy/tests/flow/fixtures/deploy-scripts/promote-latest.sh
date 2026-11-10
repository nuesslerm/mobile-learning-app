set -eu
: "${DOCKERHUB_TOKEN:?DOCKERHUB_TOKEN is not set}"; printf %s "$DOCKERHUB_TOKEN" | docker login -u "${DOCKERHUB_USERNAME:-nuesslerm}" --password-stdin >/dev/null || { echo "[vm-build] docker login failed" >&2; exit 1; }; echo "[vm-build] docker login ok"
docker buildx imagetools create -t "docker.io/nuesslerm/vm-ops-api-rs:latest" "docker.io/nuesslerm/vm-ops-api-rs:sha-abc1234"
echo "[vm-build] docker.io/nuesslerm/vm-ops-api-rs:latest -> sha-abc1234"
docker buildx imagetools create -t "docker.io/nuesslerm/watchdog:latest" "docker.io/nuesslerm/watchdog:sha-abc1234"
echo "[vm-build] docker.io/nuesslerm/watchdog:latest -> sha-abc1234"
echo "[vm-build] promoted tag=sha-abc1234"
