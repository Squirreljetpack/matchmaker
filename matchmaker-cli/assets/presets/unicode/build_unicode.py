#!/usr/bin/env python3
import urllib.request
import os
import sys
import html.entities

UNICODE_DATA_URL = "https://www.unicode.org/Public/UCD/latest/ucd/UnicodeData.txt"
LATEX_MAP_URL = "https://raw.githubusercontent.com/latex3/unicode-math/master/unicode-math-table.tex"

def download(url, filename):
    if not os.path.exists(filename):
        print(f"Downloading {filename}...", file=sys.stderr)
        urllib.request.urlretrieve(url, filename)
    else:
        print(f"Using existing {filename}", file=sys.stderr)

def main():
    download(UNICODE_DATA_URL, "UnicodeData.txt")
    download(LATEX_MAP_URL, "unicode-math-table.tex")

    latex_map = {}
    print("Processing latex mappings...", file=sys.stderr)
    with open("unicode-math-table.tex", "r") as f:
        for line in f:
            if "\\UnicodeMathSymbol" in line:
                # Format: \UnicodeMathSymbol{"02211}{\sum}{\mathop}{summation operator}%
                parts = line.split("{")
                if len(parts) >= 3:
                    hex_code = parts[1].strip('"').strip('}').strip()
                    latex_cmd = parts[2].strip("}").strip()
                    # Normalize hex
                    try:
                        hex_val = int(hex_code, 16)
                        latex_map[hex_val] = latex_cmd
                    except ValueError:
                        continue

    # Prepare HTML entities
    html_entities = html.entities.codepoint2name

    print("Processing Unicode data...", file=sys.stderr)
    output_lines = []
    with open("UnicodeData.txt", "r") as f:
        for line in f:
            parts = line.split(";")
            if len(parts) < 3:
                continue
            
            hex_str = parts[0]
            name = parts[1]
            category = parts[2]
            
            # Filter: Skip control chars, require name
            if name.startswith("<") and name.endswith(">"):
                continue
            if not name:
                continue
            
            hex_val = int(hex_str, 16)
            try:
                char = chr(hex_val)
            except ValueError:
                continue
                
            # Priority: LaTeX > HTML Entity
            latex = latex_map.get(hex_val)
            if latex:
                metadata = latex
            else:
                html_entity = html_entities.get(hex_val)
                if html_entity:
                    metadata = f"&{html_entity};"
                else:
                    metadata = ""
            
            output_lines.append(f"{char}\t{name}\t{category}\t{metadata}\n")

    print(f"Processing {len(output_lines)} lines...", file=sys.stderr)
    for line in output_lines:
        sys.stdout.write(line)

if __name__ == "__main__":
    main()
