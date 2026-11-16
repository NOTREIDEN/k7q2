#!/bin/bash
# Generate every script in scenarios.txt, record it in the real game, replay it
# against the simulator and, if asked, store the recording as a test.
#   MCP=/path/to/mcp-919 ./record_all.sh [--keep]
HERE="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$HERE/work/scripts" "$HERE/work/traces"
grep -v '^#' "$HERE/scenarios.txt" | while read -r name gen seed opts; do
  [ -z "$name" ] && continue
  python3 "$HERE/gen_script.py" "$gen" "$seed" "$HERE/work/scripts/$name.txt" $opts
  "$HERE/run.sh" "$HERE/work/scripts/$name.txt" "$HERE/work/traces/$name.log"
  [ "$1" = "--keep" ] && gzip -9 -c "$HERE/work/traces/$name.log" > "$HERE/../sim/tests/traces/$name.log.gz"
done
(cd "$HERE/../sim" && cargo run --release --example parity -- "$HERE"/work/traces/*.log)
