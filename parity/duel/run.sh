#!/bin/bash
# Two real clients fight in one world and the view of the one that joins is recorded.
#   MCP=/path/to/mcp-919 AGENT=agent.bin RIVAL=rival.bin ./run.sh out.mp4
# AGENT plays on the client that joins through the line with a wait on it (DELAY ms each
# way, 100 if not set); RIVAL plays on the host. More: KB (servidor or vanilla), ROUNDS,
# SECONDS (the longest a round lasts), RIVAL_VELOCITY (share of the horizontal knockback the
# rival takes), RIVAL_SPEED (level of its Speed from 0; 1 is Speed II, 9 Speed X),
# RIVAL_NAME, LIMIT (seconds before everything is stopped), LAYOUT (otro: the agent's kit in
# other slots, with the Speed in the inventory), SPEED_SECONDS (how long the Speed of the kit
# lasts), FOOD (the food a round starts with).
HERE="$(cd "$(dirname "$0")" && pwd)"
P="$HERE/.."
: "${MCP:?set MCP to the MCP-919 workspace}"
: "${AGENT:?set AGENT to the file of the policy that is recorded}"
JAVA="${JAVA8_HOME:-/usr/lib/jvm/java-8-openjdk-amd64}/bin/java"
OUT="$(realpath -m "${1:-duel.mp4}")"
W="$HERE/work/run"
rm -rf "$W" "$MCP"/jars/saves/duel*
mkdir -p "$W" "$(dirname "$OUT")"
CP="$HERE/work/classes:$P/work/classes:$(cat "$P/work/libs.txt")$MCP/jars/versions/1.8.9/1.8.9.jar"
PORT="${PORT:-25580}"
unset JAVA_TOOL_OPTIONS
export LIBGL_ALWAYS_SOFTWARE=1
cd "$MCP/jars"

Xvfb :93 -screen 0 854x480x24 -nolisten tcp > "$W/x93.log" 2>&1 &
X1=$!
Xvfb :94 -screen 0 320x240x24 -nolisten tcp > "$W/x94.log" 2>&1 &
X2=$!
stop() {
  kill -INT "$REC" 2>/dev/null
  for i in 1 2 3 4 5 6 7 8 9 10; do kill -0 "$REC" 2>/dev/null || break; sleep 0.5; done
  kill "$J1" "$J2" "$X1" "$X2" "$REC" 2>/dev/null
}
trap stop EXIT
until [ -e /tmp/.X11-unix/X93 ] && [ -e /tmp/.X11-unix/X94 ]; do sleep 0.2; done

DISPLAY=:94 "$JAVA" -Xmx900M -Dduel.role=host -Dduel.port="$PORT" -Dduel.dir="$W" ${RIVAL:+-Dduel.policy="$RIVAL"} \
  -Dduel.delay="${DELAY:-100}" -Dduel.kb="${KB:-servidor}" -Dduel.rounds="${ROUNDS:-3}" -Dduel.seconds="${SECONDS_MAX:-90}" \
  -Dduel.hostspeed="${RIVAL_SPEED:-1}" -Dduel.velocity="${RIVAL_VELOCITY:-1}" -Dduel.cps="${RIVAL_CPS:-20}" -Dduel.view=5 \
  -Dduel.layout="${LAYOUT:-}" -Dduel.speedsecs="${SPEED_SECONDS:-60}" -Dduel.food="${FOOD:-20}" -Dduel.floor="${FLOOR_KIND:-}" -Dduel.extra="${EXTRA:-0}" -Dduel.extramode="${EXTRA_MODE:-pegan}" \
  -Dduel.ping=10 -Dduel.otherping=$(( 2 * ${DELAY:-100} )) \
  -cp "$CP" -Djava.library.path="${LWJGL_NATIVES:-/usr/lib/jni}" Start --username "${RIVAL_NAME:-Espejo}" --width 320 --height 240 > "$W/host.log" 2>&1 &
J1=$!
END=$(( $(date +%s) + ${LIMIT:-900} ))
until [ -e "$W/ready" ]; do
  sleep 1
  if ! kill -0 "$J1" 2>/dev/null || [ "$(date +%s)" -gt "$END" ]; then echo "the host did not come up"; tail -n 30 "$W/host.log"; exit 1; fi
done
DISPLAY=:93 "$JAVA" -Xmx900M -Dduel.role=join -Dduel.port="$(cat "$W/ready")" -Dduel.dir="$W" -Dduel.policy="$AGENT" \
  -Dduel.cps="${AGENT_CPS:-14-17}" -Dduel.view=5 -Dduel.ping=$(( 2 * ${DELAY:-100} )) -Dduel.otherping=10 \
  -cp "$CP" -Djava.library.path="${LWJGL_NATIVES:-/usr/lib/jni}" Start --username "${AGENT_NAME:-PvPAI}" --width 854 --height 480 > "$W/join.log" 2>&1 &
J2=$!
until [ -e "$W/join-in" ]; do
  sleep 1
  if ! kill -0 "$J2" 2>/dev/null || [ "$(date +%s)" -gt "$END" ]; then echo "nobody joined"; tail -n 30 "$W/join.log"; exit 1; fi
done
ffmpeg -loglevel error -y -f x11grab -draw_mouse 0 -framerate 30 -video_size 854x480 -i :93 -c:v libx264 -preset ultrafast -crf 20 -pix_fmt yuv420p "$OUT" > "$W/ffmpeg.log" 2>&1 &
REC=$!
date +%s%3N > "$W/rec_start"
until [ -e "$W/done" ]; do
  sleep 1
  if ! kill -0 "$J1" 2>/dev/null || ! kill -0 "$J2" 2>/dev/null || [ "$(date +%s)" -gt "$END" ]; then echo "stopped early"; break; fi
done
grep -h "^duel: " "$W/host.log" "$W/join.log" | tail -n 40
echo "recorded $OUT"
