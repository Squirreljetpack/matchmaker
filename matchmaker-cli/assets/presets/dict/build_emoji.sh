#!/bin/sh
set -e

RENDER=${RENDER:-true}
NO_ZWJ=${NO_ZWJ:-true}

if $RENDER; then
EMOJI_CATEGORIES_URL=https://raw.githubusercontent.com/chalda-pnuzig/emojis.json/master/src/categories.json # modifiers break
dest=emoji
else
EMOJI_CATEGORIES_URL=https://raw.githubusercontent.com/chalda-pnuzig/emojis.json/master/src/categories.with.modifiers.json 
dest=emoji_compat
fi

if [ ! -f $dest.json ]; then
    echo "Downloading $EMOJI_CATEGORIES_URL..."
    curl -L $EMOJI_CATEGORIES_URL -o $dest.json
else
    echo "Using existing $dest.json"
fi

rm -f $dest.zst 2>/dev/null
echo "Processing emoji data..."
if [ "$RENDER" = "true" ]; then
    # Using jq to flatten and filter. Output: char\tname\tgroup\tsubgroup
    jq -r --arg no_zwj "$NO_ZWJ" '.emojis | to_entries[] | .key as $cat | .value | to_entries[] | .key as $sub | .value[] |
        select(.emoji != null and .name != null and (if $no_zwj == "true" then (.emoji | contains("\u200d") | not) else true end)) |
        "\(.emoji)\t\(.name)\t\($cat)\t\($sub)"' $dest.json | zstd -22 --ultra -f -o $dest.zst
else
    jq -r '.emojis | to_entries[] | .key as $cat | .value | to_entries[] | .key as $sub | .value[] | 
        select(.emoji != null and .name != null) |
        "\((.code | join("-")))\t\(.name)\t\($cat)\t\($sub)"' $dest.json | zstd -22 --ultra -f -o $dest.zst
fi

echo "Created $dest.zst"
