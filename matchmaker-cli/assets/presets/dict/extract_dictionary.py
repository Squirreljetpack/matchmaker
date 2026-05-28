import json
import sys
import os
import argparse
import subprocess
import re

DEFAULT_ZSTD_LEVEL = 10

_ws_re = re.compile(r"\s+")

def preprocess(text):
    if not text:
        return ""
    # newline handling first
    return text.replace("\n", ". ")

def clean_field(text):
    if not text:
        return ""
    # normalize all whitespace to single space + trim
    return _ws_re.sub(" ", text).strip()

def normalize(text):
    return clean_field(preprocess(text))

def get_associated(data, field_name):
    words = set()
    # check top-level
    for item in data.get(field_name, []):
        word = item.get("word", "")
        if word:
            words.add(normalize(word))
    
    # check senses
    for sense in data.get("senses", []):
        for item in sense.get(field_name, []):
            word = item.get("word", "")
            if word:
                words.add(normalize(word))
                
    return ", ".join(sorted(list(words)))

# Allowed characters: Alphanumeric (Latin) + Hyphen + Apostrophe + Space
# Includes letters, digits, and common diacritics (Latin-1/Extended-A)
# Specifically excludes broad ASCII symbols like !, ?, ., etc.
_allowed_chars_re = re.compile(
    r"^[a-zA-Z0-9\-\'\s"
    r"\u00C0-\u00D6\u00D8-\u00F6\u00F8-\u017F]+$"
)
# At least one alphabetic character (including Latin-1/Extended-A)
_has_alpha_re = re.compile(r'[a-zA-Z\u00C0-\u00D6\u00D8-\u00F6\u00F8-\u017F]')

def is_allowed_word(word):
    if not word:
        return False
    if not _allowed_chars_re.match(word):
        return False
    # No apostrophe at start or end
    if word.startswith("'") or word.endswith("'"):
        return False
    return True

def process_dictionary(input_path, output_path, columns, compression_level, use_xz=False):
    compressor = "xz" if use_xz else "zstd"
    print(
        f"Processing {input_path} -> {output_path} ({compressor} level {compression_level})...",
        file=sys.stderr
    )

    try:
        if use_xz:
            # xz -9e for maximum compression, reading from stdin and writing to output_path
            cmd = ['xz', f'-{compression_level}e', '-c']
            zstd_proc = subprocess.Popen(
                cmd,
                stdin=subprocess.PIPE,
                stdout=open(output_path, 'wb'),
                text=True,
                encoding='utf-8'
            )
        else:
            cmd = ['zstd', f'-{compression_level}', '-o', output_path]
            if compression_level > 19:
                cmd.append('--ultra')
            zstd_proc = subprocess.Popen(
                cmd,
                stdin=subprocess.PIPE,
                text=True,
                encoding='utf-8'
            )
    except FileNotFoundError:
        print(f"Error: '{compressor}' command not found.", file=sys.stderr)
        sys.exit(1)

    try:
        with open(input_path, 'r', encoding='utf-8') as f:
            for line in f:
                try:
                    data = json.loads(line)

                    raw_word = data.get("word", "")
                    if not is_allowed_word(raw_word):
                        continue

                    name = normalize(raw_word)
                    pos = normalize(data.get("pos", ""))

                    definitions = []
                    for sense in data.get("senses", []):
                        for gloss in sense.get("glosses", []):
                            g = normalize(gloss)
                            # Must contain at least one alphabetic character
                            if g and _has_alpha_re.search(g):
                                definitions.append(g)

                    if not definitions:
                        continue

                    # Filter out words where every definition starts with "plural of"
                    if all(d.lower().startswith("plural of") for d in definitions):
                        continue

                    definitions_str = "\n".join(definitions)

                    extra_data = []
                    for col in columns:
                        if col == "etymology_text":
                            extra_data.append(normalize(data.get("etymology_text", "")))
                        else:
                            extra_data.append(get_associated(data, col))

                    row_parts = [name, pos, definitions_str] + extra_data
                    zstd_proc.stdin.write("\t".join(row_parts) + "\0")

                except json.JSONDecodeError:
                    continue

        zstd_proc.stdin.close()
        zstd_proc.wait()

        if zstd_proc.returncode != 0:
            print(f"{compressor} exited with code {zstd_proc.returncode}", file=sys.stderr)
            sys.exit(zstd_proc.returncode)

        print(f"Done. Output saved to {output_path}", file=sys.stderr)

    except KeyboardInterrupt:
        zstd_proc.terminate()
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        if zstd_proc.poll() is None:
            zstd_proc.terminate()
        sys.exit(1)

if __name__ == "__main__":
    parser = argparse.ArgumentParser(
        description="Extract Kaikki dictionary data to compressed TSV.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Filters applied during extraction:
  1. Characters:
     - Allows Alphanumeric (Latin) and Latin-1/Extended-A (accents, umlauts, etc.).
     - Allows hyphens (-) and internal apostrophes (').
     - Skips words with other scripts (Cyrillic, Greek, Asian, etc.).
     - REJECTS words with ASCII symbols like !, ?, ., etc.
     - REJECTS words starting or ending with an apostrophe (e.g., 'tis, bird').
  2. Definitions:
     - Skips definitions that do not contain at least one alphabetic character.
     - Skips words where EVERY definition starts with "plural of" (case-insensitive).
     - If all definitions for a word are filtered out, the entire word is skipped.
        """
    )

    parser.add_argument("input", help="Path to the .jsonl dictionary file")

    parser.add_argument(
        "-o",
        "--output",
        help="Output file path (default: <input_basename>.<ext>)"
    )

    parser.add_argument(
        "-c",
        "--columns",
        nargs="+",
        action="extend",
        default=[],
        help=(
            "Extra columns to extract. Supported: "
            "synonyms, antonyms, hypernyms, hyponyms, meronyms, holonyms, "
            "related, derived, coordinate_terms, instances, descendants, "
            "translations, etymology_text"
        )
    )

    parser.add_argument(
        "-z",
        "--level",
        type=int,
        metavar="LEVEL",
        help="Compression level (default: 10 for zstd, 9 for xz)"
    )

    parser.add_argument(
        "--xz",
        action="store_true",
        help="Use xz compression instead of zstd (uses maximum -9e settings)"
    )

    args = parser.parse_args()

    if not os.path.exists(args.input):
        print(f"Error: File {args.input} does not exist.", file=sys.stderr)
        sys.exit(1)

    compression_level = args.level
    if compression_level is None:
        compression_level = 9 if args.xz else DEFAULT_ZSTD_LEVEL

    output_path = args.output
    if not output_path:
        base_name = os.path.splitext(os.path.basename(args.input))[0]
        ext = "xz" if args.xz else "zst"
        output_path = f"{base_name}.{ext}"

    process_dictionary(
        args.input,
        output_path,
        args.columns,
        compression_level,
        use_xz=args.xz
    )