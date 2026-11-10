set -e
echo "[redeploy] start $(date +%s)"
trap 'rm -rf /home/ubuntu/vm-ops-api/.smoke-staging' EXIT
rm -rf /home/ubuntu/vm-ops-api/.smoke-staging
mkdir -p /home/ubuntu/vm-ops-api/.smoke-staging
echo '[provision] refreshing vm-ops-api'"'"'s own compose file and .env.defaults from dev'
vm-ops-api-rs provision 'https://raw.githubusercontent.com/nuesslerm/mobile-learning-app/dev/infra/oracle-cloud/vm-ops-api/docker-compose.yml' '/home/ubuntu/vm-ops-api/.smoke-staging/docker-compose.yml' 'https://raw.githubusercontent.com/nuesslerm/mobile-learning-app/dev/infra/oracle-cloud/vm-ops-api/.env.defaults' '/home/ubuntu/vm-ops-api/.smoke-staging/.env.defaults'
if [ -f /home/ubuntu/vm-ops-api/.env.local ]; then grep -E '^(BWS_[A-Z0-9_]+|SECRETS_BACKEND)=' /home/ubuntu/vm-ops-api/.env.local > /home/ubuntu/vm-ops-api/.smoke-staging/.env.bootstrap || true; if sh -n /home/ubuntu/vm-ops-api/.smoke-staging/.env.bootstrap; then set -a; . /home/ubuntu/vm-ops-api/.smoke-staging/.env.bootstrap; set +a; rm -f /home/ubuntu/vm-ops-api/.smoke-staging/.env.bootstrap; else rm -f /home/ubuntu/vm-ops-api/.smoke-staging/.env.bootstrap; echo '[env] .env.local has a BWS_ or SECRETS_BACKEND line that does not parse' >&2; false; fi; fi
unset BETTERSTACK_OTLP_TOKEN BETTERSTACK_OTLP_HOST EXPO_PUBLIC_BETTERSTACK_TOKEN
( cd /home/ubuntu/vm-ops-api/.smoke-staging && wf secret sync-env --for vm-ops-api --backend bitwarden --base .env.defaults --out .env )
cp /home/ubuntu/vm-ops-api/.smoke-staging/.env /home/ubuntu/vm-ops-api/.smoke-staging/.env.synced
echo "HOST_LABEL=main" >> /home/ubuntu/vm-ops-api/.smoke-staging/.env
vm-ops-api-rs append-env-local /home/ubuntu/vm-ops-api/.smoke-staging/.env /home/ubuntu/vm-ops-api/.env.local
vm-ops-api-rs verify-env /home/ubuntu/vm-ops-api/.smoke-staging/.env.synced /home/ubuntu/vm-ops-api/.smoke-staging/.env
( cd /home/ubuntu/vm-ops-api/.smoke-staging && docker compose config -q )
( cd /home/ubuntu/vm-ops-api/.smoke-staging && docker compose pull ops-api )
echo "[redeploy] pulled $(date +%s)"
echo "[redeploy] smoke start $(date +%s)"
docker rm -f vm-ops-api-smoke >/dev/null 2>&1 || true
cd /home/ubuntu/vm-ops-api/.smoke-staging && docker compose run -d --no-deps --name vm-ops-api-smoke -e PORT=18999 -e CI_PORT=18998 ops-api >/dev/null
SMOKE_OK=0
for i in $(seq 1 30); do [ "$(docker inspect -f '{{.State.Running}}' vm-ops-api-smoke 2>/dev/null)" = true ] || break; if docker exec vm-ops-api-smoke sh -c 'vm-ops-api-rs --healthcheck' >/dev/null 2>&1; then SMOKE_OK=1; break; fi; sleep 2; done
if [ "$SMOKE_OK" != 1 ]; then echo '[smoke] vm-ops-api did not answer on port 18999; nothing was swapped. Its log:' >&2; docker logs --tail 80 vm-ops-api-smoke >&2; docker rm -f vm-ops-api-smoke >/dev/null 2>&1; exit 1; fi
echo '[smoke] vm-ops-api healthy on port 18999'
echo "[redeploy] smoke done $(date +%s)"