#!/usr/bin/env bash
# Bring share/pool in step with the folder "pool" of the branch "weights":
# the earlier versions of the agent that the league plays against
# (python/pvpai/league.py). Run from the root of the repository.
git fetch -q --depth 1 origin weights 2>/dev/null || exit 0
mkdir -p share/pool share/pool.tmp
names=$(git ls-tree --name-only FETCH_HEAD pool/ | sed 's#^pool/##')
for n in $names; do
  if [ ! -f "share/pool/$n" ]; then
    # Moved into place whole, so that a trainer never reads half a file.
    git cat-file blob "FETCH_HEAD:pool/$n" > "share/pool.tmp/$n" && mv -f "share/pool.tmp/$n" "share/pool/$n"
  fi
done
for f in share/pool/*.pt; do
  [ -e "$f" ] || continue
  echo "$names" | grep -qxF "$(basename "$f")" || rm -f "$f"
done
echo "pool: $(ls share/pool | tr '\n' ' ')"
