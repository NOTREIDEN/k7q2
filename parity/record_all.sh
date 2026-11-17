#!/bin/bash
# Generate the scripts in scenarios.txt (all of them, or only the scenes named),
# record each in the real game, replay it against the simulator and, if asked,
# store the recording as a test. A scene whose recording never began is started
# again, up to three times.
#   MCP=/path/to/mcp-919 ./record_all.sh [--keep] [NAME...]
# With RIG_AGENT=view the mod's own code reads the client and presses the keys, and
# the recording of a scene is called view_NAME. The scenes called bot* are for a
# policy at the keys and are left out unless RIG_AGENT is the file of one
# (python/export_policy.py writes it), which then plays those and no others.
HERE="$(cd "$(dirname "$0")" && pwd)"
KEEP=""
[ "$1" = "--keep" ] && KEEP=1 && shift
mkdir -p "$HERE/work/scripts" "$HERE/work/traces"
DONE=()
while read -r name gen seed opts; do
  [ -z "$name" ] && continue
  [ $# -gt 0 ] && [[ " $* " != *" $name "* ]] && continue
  POLICY=""
  [ -n "$RIG_AGENT" ] && [ "$RIG_AGENT" != view ] && POLICY=1
  [[ "$name" == bot* ]] && [ -z "$POLICY" ] && continue
  [[ "$name" != bot* ]] && [ -n "$POLICY" ] && continue
  python3 "$HERE/gen_script.py" "$gen" "$seed" "$HERE/work/scripts/$name.txt" $opts
  as="$name"
  [ "$RIG_AGENT" = view ] && as="view_$name"
  for try in 1 2 3; do
    "$HERE/run.sh" "$HERE/work/scripts/$name.txt" "$HERE/work/traces/$as.log" 400 < /dev/null
    grep -q "^END" "$HERE/work/traces/$as.log" && break
  done
  [ -n "$KEEP" ] && gzip -9 -c "$HERE/work/traces/$as.log" > "$HERE/../sim/tests/traces/$as.log.gz"
  DONE+=("$HERE/work/traces/$as.log")
done < <(grep -v '^#' "$HERE/scenarios.txt")
(cd "$HERE/../sim" && cargo run --release --example parity -- "${DONE[@]}")
