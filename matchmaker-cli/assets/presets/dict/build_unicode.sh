#!/bin/sh
set -e

if [ ! -f UnicodeData.txt ]; then
    echo "Downloading UnicodeData.txt..."
    curl -L https://www.unicode.org/Public/UCD/latest/ucd/UnicodeData.txt -o UnicodeData.txt
else
    echo "Using existing UnicodeData.txt"
fi

echo "Processing Unicode data..."
# Filter: Skip control chars, require name. Output: char;name;category
awk -F';' '
BEGIN { OFS=";" }
$2 !~ /^<.*>$/ && $2 != "" {
    cmd = "printf \"\\U" $1 "\""
    if ((cmd | getline char) > 0) {
        if (char != "") print char, $2, $3
    }
    close(cmd)
}' UnicodeData.txt | zstd -22 --ultra -f -o unicode.zst

echo "Created unicode.zst"
