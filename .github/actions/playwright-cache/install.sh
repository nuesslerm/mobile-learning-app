#!/usr/bin/env bash
# Chromium for the Playwright harness and the system packages it needs: from the restored .deb files (dpkg, no mirror) when the cache held them, else from the
# mirror with each try bounded, keeping the downloaded .deb files for the cache save. Runs from the workspace root; exits 0 when the browser is installed.
# `install --with-deps` checks the browser and the system packages on every call: only a new Playwright version needs it.
version=$(pnpm --filter @lib/browser-client exec playwright --version | tr -c '0-9a-z.\n' '_')
marker="$HOME/.cache/ms-playwright/.installed-$version"
[ -f "$marker" ] && exit 0
sudo=$([ "$(id -u)" = 0 ] || echo sudo)
debs="$HOME/playwright-debs"
# a restored cache holds the browser and the .deb files, never the marker: the system packages are this machine's own to install
if ls "$debs"/*.deb >/dev/null 2>&1; then
  if $sudo dpkg -i "$debs"/*.deb && timeout -k 10 150 pnpm --filter @lib/browser-client exec playwright install chromium; then
    mkdir -p "$(dirname "$marker")" && touch "$marker"; echo "playwright: installed from the cache"; exit 0
  fi
  echo "playwright: the cached packages did not install; the mirror path follows"
  $sudo dpkg --configure -a || true
fi
# a try keeps the .deb files it downloads, for the cache save
mkdir -p "$debs/partial" && chmod -R a+rwX "$debs"
printf 'Dir::Cache::Archives "%s";\nAPT::Keep-Downloaded-Packages "true";\n' "$debs" | $sudo tee /etc/apt/apt.conf.d/81ci-debs >/dev/null
# apt can stall on a mirror with no output for the rest of the job: bound each try, kill the stuck apt, try again
# a try is bounded to 150 s (the system packages take 20 to 40 s when the mirror answers; one stalled shard of run 44591379311 lost 457 s to a 420 s bound),
# and after the first failure the sources point at one mirror directly, as hosted-apt does
current="mirror+file:/etc/apt/apt-mirrors.txt"
mirrors=(http://mirrors.edge.kernel.org/ubuntu/ http://archive.ubuntu.com/ubuntu/)
for attempt in 1 2 3; do
  if timeout -k 10 150 pnpm --filter @lib/browser-client exec playwright install --with-deps chromium; then
    mkdir -p "$(dirname "$marker")" && touch "$marker"; exit 0
  fi
  echo "playwright: attempt $attempt failed or stalled"
  $sudo pkill -9 -x apt-get || true; $sudo pkill -9 -x dpkg || true; $sudo dpkg --configure -a || true
  next="${mirrors[$((attempt - 1))]:-}"
  if [ -n "$next" ]; then
    echo "playwright: switching the apt sources to $next"
    for f in /etc/apt/sources.list /etc/apt/sources.list.d/*.sources /etc/apt/sources.list.d/*.list; do
      [ -f "$f" ] && $sudo sed -i "s#${current}#${next}#g" "$f"
    done
    current="$next"
  fi
done
exit 1
