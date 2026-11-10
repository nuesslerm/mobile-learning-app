#!/usr/bin/env bash

set -euo pipefail

archive_root="$(dirname "$CONDUCTOR_WORKSPACE_PATH")/archive"
mkdir -p "$archive_root"

archive_name="$CONDUCTOR_WORKSPACE_NAME"
dest="$archive_root/$archive_name"

if [ -e "$dest" ]; then
  archive_name="$CONDUCTOR_WORKSPACE_NAME-$(date +%Y%m%d-%H%M%S)"
  dest="$archive_root/$archive_name"
fi

mv "$CONDUCTOR_WORKSPACE_PATH" "$dest"
