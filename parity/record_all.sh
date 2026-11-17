#!/bin/bash
# Generate the scripts in scenarios.txt (all of them, or only the scenes named),
# record each in the real game, replay it against the simulator and, if asked,
# store the recording as a test. A scene whose recording never began is started
# again, up to three times.
#   MCP=/path/to/mcp-919 ./record_all.sh [--keep] [NAME...]
HERE="$(cd "$(dirname "$0")" && pwd)"
KEEP=""
[ "$1" = "--keep" ] && KEEP=1 && shift
mkdir -p "$HERE/work/scripts" "$HERE/work/traces"
DONE=()
while read -r name gen seed opts; do
  [ -z "$name" ] && continue
  [ $# -gt 0 ] && [[ " $* " != *" $name "* ]] && continue
  python3 "$HERE/gen_script.py" "$gen" "$seed" "$HERE/work/scripts/$name.txt" $opts
  for try in 1 2 3; do
    "$HERE/run.sh" "$HERE/work/scripts/$name.txt" "$HERE/work/traces/$name.log" 400 < /dev/null
    grep -q "^END" "$HERE/work/traces/$name.log" && break
  done
  [ -n "$KEEP" ] && gzip -9 -c "$HERE/work/traces/$name.log" > "$HERE/../sim/tests/traces/$name.log.gz"
  DONE+=("$HERE/work/traces/$name.log")
done < <(grep -v '^#' "$HERE/scenarios.txt")
(cd "$HERE/../sim" && cargo run --release --example parity -- "${DONE[@]}")
