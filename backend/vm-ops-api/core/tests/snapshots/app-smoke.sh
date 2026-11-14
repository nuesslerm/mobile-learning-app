set -e
echo "[redeploy] start $(date +%s)"
mkdir -p /home/ubuntu/leetcode-auth-cookie-capture
echo '[provision] refreshing leetcode-auth-cookie-capture'"'"'s compose file and .env.defaults from dev'
vm-ops-api-rs provision 'https://raw.githubusercontent.com/nuesslerm/mobile-learning-app/dev/backend/leetcode-auth-cookie-capture/docker-compose.capture.yml' '/home/ubuntu/leetcode-auth-cookie-capture/docker-compose.capture.yml' 'https://raw.githubusercontent.com/nuesslerm/mobile-learning-app/dev/backend/leetcode-auth-cookie-capture/.env.defaults' '/home/ubuntu/leetcode-auth-cookie-capture/.env.defaults'
echo "[redeploy] provisioned $(date +%s)"
if [ ! -f /home/ubuntu/leetcode-auth-cookie-capture/.env.local ] && [ -f /home/ubuntu/vm-ops-api/.env.local ]; then cp /home/ubuntu/vm-ops-api/.env.local /home/ubuntu/leetcode-auth-cookie-capture/.env.local; fi
if [ -f /home/ubuntu/leetcode-auth-cookie-capture/.env.local ]; then grep -E '^(BWS_[A-Z0-9_]+|SECRETS_BACKEND)=' /home/ubuntu/leetcode-auth-cookie-capture/.env.local > /home/ubuntu/leetcode-auth-cookie-capture/.env.bootstrap || true; if sh -n /home/ubuntu/leetcode-auth-cookie-capture/.env.bootstrap; then set -a; . /home/ubuntu/leetcode-auth-cookie-capture/.env.bootstrap; set +a; rm -f /home/ubuntu/leetcode-auth-cookie-capture/.env.bootstrap; else rm -f /home/ubuntu/leetcode-auth-cookie-capture/.env.bootstrap; echo '[env] .env.local has a BWS_ or SECRETS_BACKEND line that does not parse' >&2; false; fi; fi
unset BETTERSTACK_OTLP_TOKEN BETTERSTACK_OTLP_HOST EXPO_PUBLIC_BETTERSTACK_TOKEN
( cd /home/ubuntu/leetcode-auth-cookie-capture && wf secret sync-env --for leetcode-auth-cookie-capture --backend bitwarden --base .env.defaults --out .env )
cp /home/ubuntu/leetcode-auth-cookie-capture/.env /home/ubuntu/leetcode-auth-cookie-capture/.env.synced
echo "HOST_LABEL=main" >> /home/ubuntu/leetcode-auth-cookie-capture/.env
vm-ops-api-rs append-env-local /home/ubuntu/leetcode-auth-cookie-capture/.env /home/ubuntu/leetcode-auth-cookie-capture/.env.local
vm-ops-api-rs verify-env /home/ubuntu/leetcode-auth-cookie-capture/.env.synced /home/ubuntu/leetcode-auth-cookie-capture/.env
echo "[redeploy] env synced $(date +%s)"
flock -s /home/ubuntu/vm-ops-api sh -c 'cd /home/ubuntu/leetcode-auth-cookie-capture && docker compose -f docker-compose.capture.yml pull'
echo "[redeploy] pulled $(date +%s)"
cd /home/ubuntu/leetcode-auth-cookie-capture && docker compose -f docker-compose.capture.yml up -d --no-deps --no-recreate $(docker compose -f docker-compose.capture.yml config --services | grep -vx app)
echo "[redeploy] smoke start $(date +%s)"
docker rm -f leetcode-auth-cookie-capture-smoke >/dev/null 2>&1 || true
cd /home/ubuntu/leetcode-auth-cookie-capture && docker compose -f docker-compose.capture.yml run -d --no-deps --name leetcode-auth-cookie-capture-smoke -e CAPTURE_LISTENER_PORT=18999 app >/dev/null
SMOKE_OK=0
for i in $(seq 1 30); do [ "$(docker inspect -f '{{.State.Running}}' leetcode-auth-cookie-capture-smoke 2>/dev/null)" = true ] || break; if docker exec leetcode-auth-cookie-capture-smoke sh -c 'leetcode-auth-cookie-capture-rs --healthcheck' >/dev/null 2>&1; then SMOKE_OK=1; break; fi; sleep 2; done
if [ "$SMOKE_OK" != 1 ]; then echo '[smoke] leetcode-auth-cookie-capture did not answer on port 18999; nothing was swapped. Its log:' >&2; docker logs --tail 80 leetcode-auth-cookie-capture-smoke >&2; docker rm -f leetcode-auth-cookie-capture-smoke >/dev/null 2>&1; exit 1; fi
echo '[smoke] leetcode-auth-cookie-capture healthy on port 18999'
echo "[redeploy] smoke done $(date +%s)"