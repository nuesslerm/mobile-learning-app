set -e
echo "[redeploy] start $(date +%s)"
mkdir -p /home/ubuntu/rca
echo '[provision] refreshing remote-chat-agent'"'"'s compose file and .env.defaults from dev'
vm-ops-api-rs provision 'https://raw.githubusercontent.com/nuesslerm/mobile-learning-app/dev/backend/remote-chat-agent/docker-compose.prod.yml' '/home/ubuntu/rca/docker-compose.prod.yml' 'https://raw.githubusercontent.com/nuesslerm/mobile-learning-app/dev/backend/remote-chat-agent/.env.defaults' '/home/ubuntu/rca/.env.defaults'
echo "[redeploy] provisioned $(date +%s)"
if [ ! -f /home/ubuntu/rca/.env.local ] && [ -f /home/ubuntu/vm-ops-api/.env.local ]; then cp /home/ubuntu/vm-ops-api/.env.local /home/ubuntu/rca/.env.local; fi
if [ -f /home/ubuntu/rca/.env.local ]; then grep -E '^(BWS_[A-Z0-9_]+|SECRETS_BACKEND)=' /home/ubuntu/rca/.env.local > /home/ubuntu/rca/.env.bootstrap || true; if sh -n /home/ubuntu/rca/.env.bootstrap; then set -a; . /home/ubuntu/rca/.env.bootstrap; set +a; rm -f /home/ubuntu/rca/.env.bootstrap; else rm -f /home/ubuntu/rca/.env.bootstrap; echo '[env] .env.local has a BWS_ or SECRETS_BACKEND line that does not parse' >&2; false; fi; fi
unset BETTERSTACK_OTLP_TOKEN BETTERSTACK_OTLP_HOST EXPO_PUBLIC_BETTERSTACK_TOKEN
( cd /home/ubuntu/rca && wf secret sync-env --for remote-chat-agent --backend bitwarden --base .env.defaults --out .env )
cp /home/ubuntu/rca/.env /home/ubuntu/rca/.env.synced
echo "HOST_LABEL=main" >> /home/ubuntu/rca/.env
vm-ops-api-rs append-env-local /home/ubuntu/rca/.env /home/ubuntu/rca/.env.local
vm-ops-api-rs verify-env /home/ubuntu/rca/.env.synced /home/ubuntu/rca/.env
echo "[redeploy] env synced $(date +%s)"
flock -s /home/ubuntu/vm-ops-api sh -c 'cd /home/ubuntu/rca && docker compose -f docker-compose.prod.yml pull agent'
echo "[redeploy] pulled $(date +%s)"
docker volume create agent-workspace >/dev/null 2>&1 || true ; docker volume create clis >/dev/null 2>&1 || true ; cd /home/ubuntu/rca && docker compose -f docker-compose.prod.yml up -d --no-deps $(docker compose -f docker-compose.prod.yml config --services | grep -vxE 'agent') ; docker compose -f docker-compose.prod.yml up -d --no-deps agent
if cp -p /home/ubuntu/rca/.env /home/ubuntu/rca/.env.tmp && echo "IMAGE_TAG=t1" >> /home/ubuntu/rca/.env.tmp && mv -f /home/ubuntu/rca/.env.tmp /home/ubuntu/rca/.env; then :; else rm -f /home/ubuntu/rca/.env.tmp; false; fi
echo "[redeploy] compose done $(date +%s)"
flock -x /home/ubuntu/vm-ops-api sh -c '[ "$(df --output=pcent / | tail -1 | tr -dc 0-9)" -lt 70 ] || { docker image prune -af --filter until=1h; docker builder prune -f --keep-storage 6GB; docker container prune -f --filter until=1h; }' >/dev/null 2>&1 || true