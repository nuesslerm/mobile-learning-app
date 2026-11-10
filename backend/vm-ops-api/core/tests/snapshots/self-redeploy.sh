{
date -u +%FT%TZ
echo 'pulling tag=t1'
OTLP_HOST=${BETTERSTACK_OTLP_HOST:-}; OTLP_TOKEN=${BETTERSTACK_OTLP_TOKEN:-}
wf log emit --service vm-ops-api --host-label "main" --attr tag=t1 --level info --message redeploy.started || true
trap 'rm -rf /home/ubuntu/vm-ops-api/.staging' EXIT
rm -rf /home/ubuntu/vm-ops-api/.staging
REDEPLOY_EXIT=0; RESTORED=none
rollback() { for f in /home/ubuntu/vm-ops-api/.env.defaults /home/ubuntu/vm-ops-api/.env /home/ubuntu/vm-ops-api/docker-compose.yml; do [ ! -f "$f.prev" ] || mv -f "$f.prev" "$f" || return 1; done; }
keep() { [ ! -f "$1" ] || { cp -p "$1" "$1.prev.tmp" && mv -f "$1.prev.tmp" "$1.prev"; } || { rm -f "$1.prev.tmp"; return 1; }; }
{
{ [ ! -f /home/ubuntu/vm-ops-api/.swap-pending ] || { echo 'a swap was interrupted: putting the previous files back'; rollback && rm -f /home/ubuntu/vm-ops-api/.swap-pending; }; } &&
mkdir -p /home/ubuntu/vm-ops-api/.staging &&
echo '[provision] refreshing vm-ops-api'"'"'s own compose file and .env.defaults from dev' &&
vm-ops-api-rs provision 'https://raw.githubusercontent.com/nuesslerm/mobile-learning-app/dev/infra/oracle-cloud/vm-ops-api/docker-compose.yml' '/home/ubuntu/vm-ops-api/.staging/docker-compose.yml' 'https://raw.githubusercontent.com/nuesslerm/mobile-learning-app/dev/infra/oracle-cloud/vm-ops-api/.env.defaults' '/home/ubuntu/vm-ops-api/.staging/.env.defaults' &&
if [ -f /home/ubuntu/vm-ops-api/.env.local ]; then grep -E '^(BWS_[A-Z0-9_]+|SECRETS_BACKEND)=' /home/ubuntu/vm-ops-api/.env.local > /home/ubuntu/vm-ops-api/.staging/.env.bootstrap || true; if sh -n /home/ubuntu/vm-ops-api/.staging/.env.bootstrap; then set -a; . /home/ubuntu/vm-ops-api/.staging/.env.bootstrap; set +a; rm -f /home/ubuntu/vm-ops-api/.staging/.env.bootstrap; else rm -f /home/ubuntu/vm-ops-api/.staging/.env.bootstrap; echo '[env] .env.local has a BWS_ or SECRETS_BACKEND line that does not parse' >&2; false; fi; fi &&
unset BETTERSTACK_OTLP_TOKEN BETTERSTACK_OTLP_HOST EXPO_PUBLIC_BETTERSTACK_TOKEN &&
( cd /home/ubuntu/vm-ops-api/.staging && wf secret sync-env --for vm-ops-api --backend bitwarden --base .env.defaults --out .env ) &&
cp /home/ubuntu/vm-ops-api/.staging/.env /home/ubuntu/vm-ops-api/.staging/.env.synced &&
echo "HOST_LABEL=main" >> /home/ubuntu/vm-ops-api/.staging/.env &&
vm-ops-api-rs append-env-local /home/ubuntu/vm-ops-api/.staging/.env /home/ubuntu/vm-ops-api/.env.local &&
vm-ops-api-rs verify-env /home/ubuntu/vm-ops-api/.staging/.env.synced /home/ubuntu/vm-ops-api/.staging/.env &&
( cd /home/ubuntu/vm-ops-api/.staging && docker compose config -q ) &&
( cd /home/ubuntu/vm-ops-api/.staging && docker compose pull ops-api )
} || REDEPLOY_EXIT=$?
if [ "$REDEPLOY_EXIT" = 0 ]; then { keep /home/ubuntu/vm-ops-api/.env.defaults && keep /home/ubuntu/vm-ops-api/.env && keep /home/ubuntu/vm-ops-api/docker-compose.yml; } || REDEPLOY_EXIT=$?; fi
if [ "$REDEPLOY_EXIT" = 0 ]; then
{
: > /home/ubuntu/vm-ops-api/.swap-pending &&
mv -f /home/ubuntu/vm-ops-api/.staging/.env.defaults /home/ubuntu/vm-ops-api/.env.defaults &&
mv -f /home/ubuntu/vm-ops-api/.staging/.env /home/ubuntu/vm-ops-api/.env &&
mv -f /home/ubuntu/vm-ops-api/.staging/docker-compose.yml /home/ubuntu/vm-ops-api/docker-compose.yml &&
rm -f /home/ubuntu/vm-ops-api/.swap-pending &&
( cd /home/ubuntu/vm-ops-api && docker compose up -d --wait --wait-timeout 120 ) &&
if cp -p /home/ubuntu/vm-ops-api/.env /home/ubuntu/vm-ops-api/.env.tmp && echo "IMAGE_TAG=t1" >> /home/ubuntu/vm-ops-api/.env.tmp && mv -f /home/ubuntu/vm-ops-api/.env.tmp /home/ubuntu/vm-ops-api/.env; then :; else rm -f /home/ubuntu/vm-ops-api/.env.tmp; false; fi
} || REDEPLOY_EXIT=$?
  if [ "$REDEPLOY_EXIT" != 0 ]; then
    if rollback; then rm -f /home/ubuntu/vm-ops-api/.swap-pending; if ( cd /home/ubuntu/vm-ops-api && unset IMAGE_TAG && docker compose up -d ); then RESTORED=ok; else RESTORED=failed; echo "restore: docker compose up -d failed"; fi; else RESTORED=failed; echo "restore: moving the .prev files back failed"; fi
  fi
fi
echo "exit=$REDEPLOY_EXIT restored=$RESTORED"
if [ "$REDEPLOY_EXIT" = "0" ]; then LOGGER_LEVEL=info; LOGGER_MSG=redeploy.succeeded; else LOGGER_LEVEL=error; LOGGER_MSG=redeploy.failed; fi
BETTERSTACK_OTLP_HOST="$OTLP_HOST" BETTERSTACK_OTLP_TOKEN="$OTLP_TOKEN" wf log emit --service vm-ops-api --host-label "main" --attr tag=t1 --level "$LOGGER_LEVEL" --message "$LOGGER_MSG" --attr exitCode="$REDEPLOY_EXIT" --attr restored="$RESTORED" || true
printf '{"ts":"%s","key":"vm-ops-api","requestedBy":"unknown","tag":"t1","ref":"dev","outcome":"%s","exitCode":%s,"restored":"%s"}\n' "$(date -u +%FT%TZ)" "$LOGGER_MSG" "$REDEPLOY_EXIT" "$RESTORED" >> redeploy-history.jsonl
tail -n 200 redeploy-history.jsonl > redeploy-history.jsonl.tmp && mv redeploy-history.jsonl.tmp redeploy-history.jsonl
exit "$REDEPLOY_EXIT"
} > last-redeploy.log 2>&1