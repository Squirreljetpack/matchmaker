#!/usr/bin/env zsh

: "${DOCKERdir:=$HOME/docker}"
: "${JQ:=jq}"

# Source prioritization: compose > pod > file
typeset -A item_statuses identifiers sources

# Helper to add items
add_item() {
    local name=$1 item_status=$2 id=$3 source=$4
    if [[ -z "${sources[$name]}" ]]; then
        sources[$name]=$source
        item_statuses[$name]=$item_status
        identifiers[$name]=$id
    fi
}

# 1. Compose (Docker & Podman)
for cmd in docker podman; do
    if (( $+commands[$cmd] )); then
        json=$($cmd compose ls --all --format json 2>/dev/null)
        if [[ -n "$json" && "$json" != "[]" ]]; then
            echo "$json" | $JQ -r '.[] | "\(.Name)\t\(.Status)\t\(.ConfigFiles)"' | while IFS=$'\t' read -r name item_status config; do
                add_item "$name" "$item_status" "$config" "compose"
            done
        fi
    fi
done

# 2. Podman Pods
if (( $+commands[podman] )); then
    json=$(podman pod ls --format json 2>/dev/null)
    if [[ -n "$json" && "$json" != "[]" ]]; then
        echo "$json" | $JQ -r '.[] | "\(.Name)\t\(.Status)\t\(.Id)"' | while IFS=$'\t' read -r name item_status id; do
            add_item "$name" "$item_status" "$id" "pod"
        done
    fi
fi

# 3. Local Filesystem (strictly compose.yml)
if [[ -d "$DOCKERdir" ]]; then
    # find all compose.yml files up to 2 levels deep
    find "$DOCKERdir" -maxdepth 2 -name "compose.yml" | while read -r file; do
        name=$(basename "$(dirname "$file")")
        add_item "$name" "down" "$file" "file"
    done
fi

# Output null-delimited
for name in ${(k)sources}; do
    printf "%s\t%s\t%s\t%s\0" "$name" "${item_statuses[$name]}" "${identifiers[$name]}" "${sources[$name]}"
done
