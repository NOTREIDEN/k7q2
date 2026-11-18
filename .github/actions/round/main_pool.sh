#!/usr/bin/env bash
# For an exploiter (exploit.yml): share/pool holds the main agent alone, as
# it is on the branch "weights" right now, so that every duel of the league
# is against it. Run from the root of the repository.
git fetch -q --depth 1 origin weights 2>/dev/null || exit 0
mkdir -p share/pool share/pool.tmp
blob=$(git rev-parse FETCH_HEAD:latest.pt 2>/dev/null) || exit 0
if [ "$blob" != "$(cat share/main.blob 2>/dev/null)" ]; then
  # Moved into place whole, so that a trainer never reads half a file.
  git cat-file blob "$blob" > share/pool.tmp/main.pt && mv -f share/pool.tmp/main.pt share/pool/main.pt && echo "$blob" > share/main.blob
fi
echo "opponent: main agent $(cat share/main.blob 2>/dev/null)"
