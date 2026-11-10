#!/usr/bin/env bash
# rtk-hook-version: 3
# RTK Claude Code hook — rewrites commands to use rtk for token savings.
# Requires: rtk >= 0.23.0, jq
#
# This is a thin delegating hook: all rewrite logic lives in `rtk rewrite`,
# which is the single source of truth (src/discover/registry.rs).
# To add or change rewrite rules, edit the Rust registry — not this file.
#
# Exit code protocol for `rtk rewrite`:
#   0 + stdout  Rewrite found, no deny/ask rule matched → auto-allow
#   1           No RTK equivalent → pass through unchanged
#   2           Deny rule matched → pass through (Claude Code native deny handles it)
#   3 + stdout  Ask rule matched → rewrite but let Claude Code prompt the user

if ! command -v jq &>/dev/null; then
  echo "[rtk] WARNING: jq is not installed. Hook cannot rewrite commands. Install jq: https://jqlang.github.io/jq/download/" >&2
  exit 0
fi

if ! command -v rtk &>/dev/null; then
  echo "[rtk] WARNING: rtk is not installed or not in PATH. Hook cannot rewrite commands. Install: https://github.com/rtk-ai/rtk#installation" >&2
  exit 0
fi

# Version guard: rtk rewrite was added in 0.23.0.
# Older binaries: warn once and exit cleanly (no silent failure).
# Cache the version check to avoid spawning multiple processes on every hook call.
CACHE_DIR=${XDG_CACHE_HOME:-$HOME/.cache}
CACHE_FILE="$CACHE_DIR/rtk-hook-version-ok"
if [ ! -f "$CACHE_FILE" ]; then
  RTK_VERSION_RAW=$(rtk --version 2>/dev/null)
  RTK_VERSION=${RTK_VERSION_RAW#rtk }
  RTK_VERSION=${RTK_VERSION%% *}
  if [ -n "$RTK_VERSION" ]; then
    IFS=. read -r MAJOR MINOR PATCH <<<"$RTK_VERSION"
    # Require >= 0.23.0
    if [ "$MAJOR" -eq 0 ] && [ "$MINOR" -lt 23 ]; then
      echo "[rtk] WARNING: rtk $RTK_VERSION is too old (need >= 0.23.0). Upgrade: cargo install rtk" >&2
      exit 0
    fi
  fi
  mkdir -p "$CACHE_DIR" 2>/dev/null
  touch "$CACHE_FILE" 2>/dev/null
fi

INPUT=$(cat)
CMD=$(jq -r '.tool_input.command // empty' <<<"$INPUT")

if [ -z "$CMD" ]; then
  exit 0
fi

# Delegate all rewrite + permission logic to the Rust binary.
REWRITTEN=$(rtk rewrite "$CMD" 2>/dev/null)
EXIT_CODE=$?

# A command containing a pipeline or redirect is NOT rewritten, and that is
# RTK's deliberate design rather than a gap to work around. RTK saves tokens by
# truncating and reformatting output for a human reader; a downstream `wc -l`,
# `jq`, `patch` or `grep` receives bytes, and those two goals are incompatible.
#
# Measured here on 2026-09-07, after briefly "fixing" this by rewriting a
# pipeline's head segment: `ls node_modules/.pnpm | wc -l` reports 3450, while
# `rtk ls node_modules/.pnpm | wc -l` reports 52 -- the rewrite silently turned
# a correct count into a wrong one, with nothing in the output to show for it.
# That change was reverted.
#
# The way to earn the saving is to write the command unpiped in the first place
# (`rtk ls dir` already truncates, so `| head` is redundant), not to rewrite
# half a pipeline.

# What CAN be fixed is the habit on this side of the hook. A pipeline whose
# only job is to trim output -- `<cmd> | head -N`, `| tail -N`, `| cat` -- asks
# a second process to do what `rtk <cmd>` already does, and pays full tokens
# for the untrimmed stream on the way. So when the head of such a pipeline has
# an RTK equivalent, block the call with an explanation instead of letting it
# run: exit 2 puts the message in front of the model, which can then re-issue
# the command without the pipe.
#
# Narrow on purpose. It fires only when the tail is purely cosmetic; `| wc -l`,
# `| jq`, `| grep`, `| patch` and everything else consume bytes, so a rewrite
# there would change the answer (`ls | wc -l` = 3450 vs `rtk ls | wc -l` = 52)
# and the command is left strictly alone. An explicit `rtk <cmd> | head -N` is
# never blocked either -- that is the escape hatch when exactly N lines are
# genuinely wanted.
if [[ "$CMD" != rtk\ * ]] && [[ "$CMD" == *"|"* ]]; then
  PIPE_HEAD=${CMD%%|*}
  PIPE_TAIL=${CMD#*|}
  # Cosmetic tail: head/tail/cat, optional numeric flag, nothing after it.
  if [[ "$PIPE_TAIL" =~ ^[[:space:]]*(head|tail)([[:space:]]+-n?[0-9]+)?[[:space:]]*$ ||
        "$PIPE_TAIL" =~ ^[[:space:]]*cat[[:space:]]*$ ]]; then
    if [[ "$PIPE_HEAD" != *"'"* && "$PIPE_HEAD" != *'"'* && "$PIPE_HEAD" != *'<<'* ]]; then
      # `2>&1` only exists to feed the pipe, and rtk has no rewrite for a command that carries one.
      PIPE_HEAD=${PIPE_HEAD//2>&1/}
      SUGGESTED=$(rtk rewrite "$PIPE_HEAD" 2>/dev/null)
      SUGGESTED_CODE=$?
      # 0 is a rewrite; 3 is a rewrite whose rule asks for confirmation (cargo, git push). Both prove an RTK form exists, and
      # treating only 0 as one let every `cargo ... | tail` through.
      if { [ "$SUGGESTED_CODE" -eq 0 ] || [ "$SUGGESTED_CODE" -eq 3 ]; } && [ -n "$SUGGESTED" ] && [ "$SUGGESTED" != "$PIPE_HEAD" ]; then
        echo "[rtk] This pipeline only trims output, and RTK cannot rewrite a piped command (its truncation would corrupt whatever reads the bytes). Run it as: ${SUGGESTED}" >&2
        echo "[rtk] That form is already truncated and costs far fewer tokens. If you genuinely need exactly the piped form, write it as \`${SUGGESTED} |${PIPE_TAIL}\` and it will pass through." >&2
        exit 2
      fi
    fi
  fi
fi

case $EXIT_CODE in
  0)
    # Rewrite found, no permission rules matched — safe to auto-allow.
    # If the output is identical, the command was already using RTK.
    [ "$CMD" = "$REWRITTEN" ] && exit 0
    ;;
  1)
    # No RTK equivalent — pass through unchanged.
    exit 0
    ;;
  2)
    # Deny rule matched — let Claude Code's native deny rule handle it.
    exit 0
    ;;
  3)
    # Ask rule matched — rewrite the command but do NOT auto-allow so that
    # Claude Code prompts the user for confirmation.
    ;;
  *)
    exit 0
    ;;
esac

if [ "$EXIT_CODE" -eq 3 ]; then
  # Ask: rewrite the command, omit permissionDecision so Claude Code prompts.
  jq -c --arg cmd "$REWRITTEN" \
    '.tool_input.command = $cmd | {
      "hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "updatedInput": .tool_input
      }
    }' <<<"$INPUT"
else
  # Allow: rewrite the command and auto-allow.
  jq -c --arg cmd "$REWRITTEN" \
    '.tool_input.command = $cmd | {
      "hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "allow",
        "permissionDecisionReason": "RTK auto-rewrite",
        "updatedInput": .tool_input
      }
    }' <<<"$INPUT"
fi
