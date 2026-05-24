#!/usr/bin/env zsh

identifier="$1"
source="$2"
: "${JQ:=jq}"

echo "$@" >> $HOME/log

# Helper to detect compose tool
get_compose_tool() {
    local id="$1"
    if [[ "$id" == *docker* ]]; then
        echo "docker"
    elif [[ "$id" == *podman* ]]; then
        echo "podman"
    elif (( $+commands[docker] )); then
        echo "docker"
    else
        echo "podman"
    fi
}

if [[ -n "$identifier" && -n "$source" ]]; then
    case "$source" in
        compose|file)
            tool=$(get_compose_tool "$identifier")
            IFS=',' read -r -A files <<< "$identifier"
            args=()
            for f in "${files[@]}"; do
                args+=("-f" "$f")
            done

            # compose ps --format json output is one JSON object per line
            $tool compose "${args[@]}" ps --all --format json 2>/dev/null | $JQ -r '"\(.Name)\t\(.Status)\t\(.ID)\t\(.Image)\t\(.State)"' | while read -r line; do
                printf "%s\0" "$line"
            done
            ;;
        pod)
            # podman ps --format json output is a JSON array
            podman ps --all --filter pod="$identifier" --format json 2>/dev/null | $JQ -r '.[] | "\(.Names[0])\t\(.Status)\t\(.Id)\t\(.Image)\t\(.State)"' | while read -r line; do
                printf "%s\0" "$line"
            done
            ;;
    esac
else
    # List all containers
    if (( $+commands[docker] )); then
        docker ps --all --format json 2>/dev/null | $JQ -r '"\(.Names)\t\(.Status)\t\(.ID)\t\(.Image)\t\(.State)"' | while read -r line; do
            printf "%s\0" "$line"
        done
    fi
    if (( $+commands[podman] )); then
        podman ps --all --format json 2>/dev/null | $JQ -r '.[] | "\(.Names[0])\t\(.Status)\t\(.Id)\t\(.Image)\t\(.State)"' | while read -r line; do
            printf "%s\0" "$line"
        done
    fi
fi
