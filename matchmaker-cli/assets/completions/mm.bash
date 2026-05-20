_mm() {
    local i cur prev opts cmd
    COMPREPLY=()
    if [[ "${BASH_VERSINFO[0]}" -ge 4 ]]; then
        cur="$2"
    else
        cur="${COMP_WORDS[COMP_CWORD]}"
    fi
    prev="$3"
    cmd=""
    opts=""

    for i in "${COMP_WORDS[@]:0:COMP_CWORD}"
    do
        case "${cmd},${i}" in
            ",$1")
                cmd="mm"
                ;;
            *)
                ;;
        esac
    done

    case "${cmd}" in
        mm)
            opts="-o -F -q -v -d -h --config --override --dump-config --test-keys --last-key --no-read --download --doc --help [ARGS]..."
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 1 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --config)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --override)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                -o)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --download)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --doc)
                    COMPREPLY=($(compgen -W "options binds template other" -- "${cur}"))
                    return 0
                    ;;
                -d)
                    COMPREPLY=($(compgen -W "options binds template other" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
    esac
}

if [[ "${BASH_VERSINFO[0]}" -eq 4 && "${BASH_VERSINFO[1]}" -ge 4 || "${BASH_VERSINFO[0]}" -gt 4 ]]; then
    complete -F _mm -o nosort -o bashdefault -o default mm
else
    complete -F _mm -o bashdefault -o default mm
fi
