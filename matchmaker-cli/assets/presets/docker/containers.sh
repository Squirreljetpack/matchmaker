#!/usr/bin/env zsh

id="$1"
source="$2"
: "${JQ:=jq}"

# Helper to detect compose tool
get_compose_tool() {
    if [[ "$id" == *docker* ]]; then
        tool="docker"
    elif [[ "$id" == *podman* ]]; then
        tool="podman"
    elif podman inspect "$id" >/dev/null 2>&1; then
        tool="podman"
    else
        tool="docker"
    fi
}

if [[ -n "$id" && -n "$source" ]]; then
    case "$source" in
        compose|file)
            get_compose_tool
            IFS=',' read -r -A files <<< "$id"
            args=()
            for f in "${files[@]}"; do
                args+=("-f" "$f")
            done

            # compose ps --format json output is one JSON object per line
            $tool compose "${args[@]}" ps --all --format json 2>/dev/null | $JQ -r '"\(.Name)\t\(.Status)\t\(.State)\t\(.Image)\t\(.Id)"'
            ;;
        pod)
            # podman ps --format json output is a JSON array
            podman ps --all --filter pod="$id" --format json 2>/dev/null | $JQ -r '.[] | "\(.Names[0])\t\(.Status)\t\(.State)\t\(.Image)\t\(.Id)"'
            ;;
    esac
else
    # List all containers
    if (( $+commands[docker] )); then
        docker ps --all --format json 2>/dev/null | $JQ -r '"\(.Names)\t\(.Status)\t\(.State)\t\(.Image)\t\(.Id)"'
    fi
    if (( $+commands[podman] )); then
        podman ps --all --format json 2>/dev/null | $JQ -r '.[] | "\(.Names[0])\t\(.Status)\t\(.State)\t\(.Image)\t\(.Id)"'
    fi
fi
