#!/usr/bin/env zsh

eval id=\${$#}
[ -n "$id" ] || exit 1

if podman inspect "$id" >/dev/null 2>&1; then
    tool="podman"
else
    tool="docker"
fi

echo $tool container "$@"
printf "%${COLUMNS}s\n" | tr ' ' '-'

$tool container "$@"

