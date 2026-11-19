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
# The newest exploiter (branch "exploit", see exploit.yml) plays in the
# league too, as exploit.pt: a new version of it starts with a clean record.
if git fetch -q --depth 1 origin exploit 2>/dev/null; then
  blob=$(git rev-parse FETCH_HEAD:latest.pt 2>/dev/null || true)
  if [ -n "$blob" ] && [ "$blob" != "$(cat share/exploit.blob 2>/dev/null)" ]; then
    git cat-file blob "$blob" > share/pool.tmp/exploit.pt && mv -f share/pool.tmp/exploit.pt share/pool/exploit.pt \
      && echo "$blob" > share/exploit.blob
  fi
  names="$names
exploit.pt"
fi
# Pinned versions kept in the repository (seed/pin_*.pt, see
# python/pvpai/league.py) play in the league whatever the ladder says.
for f in seed/pin_*.pt; do
  [ -e "$f" ] || continue
  n=$(basename "$f")
  cmp -s "$f" "share/pool/$n" || { cp "$f" "share/pool.tmp/$n" && mv -f "share/pool.tmp/$n" "share/pool/$n"; }
  names="$names
$n"
done
for f in share/pool/*.pt; do
  [ -e "$f" ] || continue
  echo "$names" | grep -qxF "$(basename "$f")" || rm -f "$f"
done
echo "pool: $(ls share/pool | tr '\n' ' ')"
