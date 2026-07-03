complete -c mm -l config -r -F
complete -c mm -s o -l override -d 'Paths without a toml extension refer to a preset' -r -F
complete -c mm -l download -d 'Download all presets from GitHub. Use `--download=<FOLDER>` to download only a subfolder' -r
complete -c mm -s d -l doc -d 'Display documentation' -r -f -a "options\t''
binds\t''
template\t''
other\t''"
complete -c mm -l dump-config -d 'Write the default configuration to the default location. If piped, writes the current configuration to stdout'
complete -c mm -s F
complete -c mm -l test-keys
complete -c mm -l last-key -d 'Print the last key pressed in the last `mm` run'
complete -c mm -l no-read -d 'Force the default command to run'
complete -c mm -s q -d 'Reduce the verbosity level'
complete -c mm -s v -d 'Increase the verbosity level'
complete -c mm -s h -l help -d 'Print help'
