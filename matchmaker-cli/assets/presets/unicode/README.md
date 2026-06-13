This preset provides a unified interface for Unicode characters and Emojis.

It processes:
- https://www.unicode.org/Public/UCD/latest/ucd/UnicodeData.txt for all single-codepoint characters.
- LaTeX mappings from https://github.com/latex3/unicode-math.
- HTML entities from the Python standard library.
- https://github.com/chalda-pnuzig/emojis.json for emojis.

The data includes a 4th column for metadata (e.g., `\sum` for `∑`, `&copy;` for `©`, or emoji subgroups).

### Usage
Run `mm -o unicode` to start. You can cycle between Unicode and Emojis using the reload keys.

### Build
To update the data:
1. Run `sh build_unicode.sh` to update `unicode.zst`.
2. Run `sh build_emoji.sh` to update `emoji.zst` and `emoji_compat.zst`.

![](https://github.com/Squirreljetpack/matchmaker/blob/main/matchmaker-lib/assets/extra/unicode.png?raw=true)

![](https://github.com/Squirreljetpack/matchmaker/blob/main/matchmaker-lib/assets/extra/emoji.png?raw=true)
