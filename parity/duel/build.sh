#!/bin/bash
# Compile the duel rig over the parity rig's build (run ../build.sh first): its own
# rig.Rig, and the arithmetic of the test server's plugin for the knockback.
#   MCP=/path/to/mcp-919 ./build.sh
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"
: "${MCP:?set MCP to the MCP-919 workspace}"
JAVAC="${JAVA8_HOME:-/usr/lib/jvm/java-8-openjdk-amd64}/bin/javac"
P="$HERE/.."
rm -rf "$HERE/work/classes"
mkdir -p "$HERE/work/classes"
(unset JAVA_TOOL_OPTIONS; "$JAVAC" -nowarn -encoding UTF-8 -source 1.8 -target 1.8 -cp "$P/work/classes:$(cat "$P/work/libs.txt")" -d "$HERE/work/classes" \
  "$HERE"/src/rig/*.java "$P/../plugin/src/main/java/pvpai/plugin/Knock.java" "$P/../plugin/src/main/java/pvpai/plugin/Profile.java" 2>&1 | grep -v "^Note:" || true)
ls "$HERE/work/classes/rig/Rig.class" > /dev/null
echo "built $HERE/work/classes"
