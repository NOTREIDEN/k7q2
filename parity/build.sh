#!/bin/bash
# Patch five decompiled sources with the hook lines and compile them with the rig.
#   MCP=/path/to/mcp-919 ./build.sh
# Needs a Java 8 JDK and an MCP-919 workspace that has been decompiled and
# recompiled once (src/minecraft, bin/minecraft, jars/).
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"
: "${MCP:?set MCP to the MCP-919 workspace}"
JAVAC="${JAVA8_HOME:-/usr/lib/jvm/java-8-openjdk-amd64}/bin/javac"
mkdir -p "$HERE/work/classes"
python3 "$HERE/patch.py" "$MCP/src/minecraft" "$HERE/work/patched"
# The workspace ships LWJGL with Windows natives only; use the 2.9.3 jars in dl/
# together with the system's natives instead.
LIBS="$MCP/bin/minecraft:$(ls "$HERE"/dl/*.jar | tr '\n' ':')$(find "$MCP/jars/libraries" -name '*.jar' | grep -v -e /org/lwjgl/ -e natives -e /tv/twitch/ | tr '\n' ':')"
echo "$LIBS" > "$HERE/work/libs.txt"
(unset JAVA_TOOL_OPTIONS; "$JAVAC" -nowarn -encoding UTF-8 -source 1.8 -target 1.8 -cp "$LIBS" -d "$HERE/work/classes" \
  $(find "$HERE/work/patched" "$HERE/src" -name '*.java') 2>&1 | grep -v "^Note:" || true)
echo "built $HERE/work/classes"
