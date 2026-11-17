#!/bin/bash
# Run the real client on a script and write the recording.
#   MCP=/path/to/mcp-919 ./run.sh scripts/fight1.txt traces/fight1.log [seconds]
# The client runs under xvfb with software rendering; nothing is shown.
# With RIG_AGENT=view the mod's own code reads the client and presses the script's keys;
# with RIG_AGENT=/path/to/policy.bin that policy plays through it.
HERE="$(cd "$(dirname "$0")" && pwd)"
: "${MCP:?set MCP to the MCP-919 workspace}"
JAVA="${JAVA8_HOME:-/usr/lib/jvm/java-8-openjdk-amd64}/bin/java"
SCRIPT="$(realpath "$1")"
OUT="$(realpath -m "$2")"
mkdir -p "$(dirname "$OUT")"
cd "$MCP/jars"
CP="$HERE/work/classes:$(cat "$HERE/work/libs.txt")$MCP/jars/versions/1.8.9/1.8.9.jar"
LIBGL_ALWAYS_SOFTWARE=1 timeout "${3:-300}" xvfb-run -a -s "-screen 0 854x480x24" "$JAVA" \
  -Xmx1024M -Drig.script="$SCRIPT" -Drig.out="$OUT" ${RIG_AGENT:+-Drig.agent="$RIG_AGENT"} -cp "$CP" -Djava.library.path="${LWJGL_NATIVES:-/usr/lib/jni}" Start > "$OUT.console" 2>&1
echo "exit $?"
