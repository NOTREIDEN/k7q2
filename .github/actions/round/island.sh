#!/usr/bin/env bash
# The newest network of the other fleet (python/pvpai/sync.py, island_dir):
# ISLAND is owner/repo:branch, and the network is the file latest.pt there.
# Fetched every round into share/island under the name of its blob, the
# newest few kept. Does nothing without ISLAND. Run from the root of the
# repository.
[ -n "$ISLAND" ] || exit 0
repo=${ISLAND%%:*}; branch=${ISLAND#*:}
[ "$branch" != "$ISLAND" ] || branch=weights
git fetch -q --depth 1 "https://github.com/$repo" "$branch" 2>/dev/null || { echo "island: nothing to fetch"; exit 0; }
blob=$(git rev-parse FETCH_HEAD:latest.pt 2>/dev/null) || exit 0
mkdir -p share/island share/island.tmp
if [ ! -f "share/island/$blob.pt" ]; then
  # Moved into place whole, so that a trainer never reads half a file.
  git cat-file blob "$blob" > "share/island.tmp/$blob.pt" && mv -f "share/island.tmp/$blob.pt" "share/island/$blob.pt"
fi
ls -t share/island/*.pt 2>/dev/null | tail -n +5 | xargs -r rm -f
echo "island: $(ls share/island | tr '\n' ' ')"
