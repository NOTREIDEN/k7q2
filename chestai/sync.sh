#!/usr/bin/env bash
# Carry weights between the training machines through one git branch per
# machine: push our own file of this round to sync/m<ours>, then wait for
# the other machines' files of the same round and copy them into the share
# folder. Usage: sync.sh SHARE_DIR MACHINE MACHINES ROUND
set -u
S=$1; M=$2; N=$3; R=$4; OUT=${5:-}
f="$S/m${M}_r${R}.pt"
W=/tmp/chestai-syncrepo
if [ ! -d "$W/.git" ]; then
  git init -q "$W"
  git -C "$W" remote add origin "$(git -C "$GITHUB_WORKSPACE" remote get-url origin)"
  h=$(git -C "$GITHUB_WORKSPACE" config --get http.https://github.com/.extraheader || true)
  [ -n "$h" ] && git -C "$W" config http.https://github.com/.extraheader "$h"
  git -C "$W" config user.name "trainer"
  git -C "$W" config user.email "trainer@users.noreply.github.com"
fi
cd "$W"
git checkout -q --orphan "w$R" 2>/dev/null || git checkout -q "w$R"
git rm -rqf --cached . >/dev/null 2>&1 || true
rm -f ./*.pt
RUN=${GITHUB_RUN_ID:-0}
cp "$f" "m${M}_r${R}_${RUN}.pt" && git add "m${M}_r${R}_${RUN}.pt"
# The first machine also carries its log, for a look at the run while it goes on.
if [ "$M" = 0 ] && [ -n "$OUT" ] && [ -f "$OUT/log.jsonl" ]; then
  cp "$OUT/log.jsonl" log.jsonl && git add log.jsonl
fi
git commit -qm "machine $M round $R"
for i in 1 2 3 4 5; do git push -qf origin "HEAD:refs/heads/chestai-sync/m$M" && break; sleep $((i * 3)); done
end=$((SECONDS + 200))
while [ $SECONDS -lt $end ]; do
  missing=0
  git fetch -q --depth 1 origin "+refs/heads/chestai-sync/*:refs/remotes/origin/chestai-sync/*" 2>/dev/null || true
  for j in $(seq 0 $((N - 1))); do
    [ "$j" = "$M" ] && continue
    out="$S/m${j}_r${R}.pt"
    [ -s "$out" ] && continue
    if git cat-file -e "origin/chestai-sync/m$j:m${j}_r${R}_${RUN}.pt" 2>/dev/null; then
      git show "origin/chestai-sync/m$j:m${j}_r${R}_${RUN}.pt" > "$out.tmp" && mv "$out.tmp" "$out"
    else
      missing=1
    fi
  done
  [ $missing = 0 ] && break
  sleep 10
done
ls "$S" | tail -n 20
