#!/usr/bin/env zsh

identifier="$1"
action="$2"
source="$3"
shift 3

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

case "$source" in
    compose|file)
        tool=$(get_compose_tool "$identifier")
        # Handle multiple config files (comma separated)
        IFS=',' read -r -A files <<< "$identifier"
        args=()
        for f in "${files[@]}"; do
            args+=("-f" "$f")
        done

        exec $tool compose "${args[@]}" "$action" "$@"
        ;;
    pod)
        pod_id="$identifier"
        pod_cmd="$action"

        case "$pod_cmd" in
            logs) ;;
            restart) ;;
            config) pod_cmd=inspect ;;
            up) pod_cmd=start ;;
            down) pod_cmd=rm ;;
            ps)
                exec podman ps --filter pod="$pod_id" "$@"
                ;;
            stats) pod_cmd=stats ;;
            top) pod_cmd=top ;;
            *) ;;
        esac

        exec podman pod "$pod_cmd" "$pod_id" "$@"
        ;;
    *)
        echo "Unknown source: $source" >&2
        exit 1
        ;;
esac
