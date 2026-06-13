#!/bin/sh
set -e

DIR="$(dirname "$0")"
cd "$DIR"

echo "Generating Unicode data with LaTeX mappings..."
python3 build_unicode.py | zstd -22 --ultra -f -o unicode.zst

echo "Created unicode.zst"

