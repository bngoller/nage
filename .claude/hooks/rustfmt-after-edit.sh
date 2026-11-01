#!/usr/bin/env bash
# Claude Code PostToolUse hook: format a Rust file right after Claude edits or writes it.
# Claude Code pipes the tool call as JSON on stdin; nothing is printed on success.
# Exit 2 sends stderr back to Claude; any other non-zero exit is only shown to the user.
set -u
file=$(jq -r '.tool_input.file_path // empty') || { echo "rustfmt hook: jq failed, file not formatted" >&2; exit 2; }
case "$file" in
  *.rs) rustfmt --edition 2024 "$file" || exit 2 ;;
  *) exit 0 ;;
esac
