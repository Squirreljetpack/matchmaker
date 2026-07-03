
using namespace System.Management.Automation
using namespace System.Management.Automation.Language

Register-ArgumentCompleter -Native -CommandName 'mm' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $commandElements = $commandAst.CommandElements
    $command = @(
        'mm'
        for ($i = 1; $i -lt $commandElements.Count; $i++) {
            $element = $commandElements[$i]
            if ($element -isnot [StringConstantExpressionAst] -or
                $element.StringConstantType -ne [StringConstantType]::BareWord -or
                $element.Value.StartsWith('-') -or
                $element.Value -eq $wordToComplete) {
                break
        }
        $element.Value
    }) -join ';'

    $completions = @(switch ($command) {
        'mm' {
            [CompletionResult]::new('--config', '--config', [CompletionResultType]::ParameterName, 'config')
            [CompletionResult]::new('-o', '-o', [CompletionResultType]::ParameterName, 'Paths without a toml extension refer to a preset')
            [CompletionResult]::new('--override', '--override', [CompletionResultType]::ParameterName, 'Paths without a toml extension refer to a preset')
            [CompletionResult]::new('--download', '--download', [CompletionResultType]::ParameterName, 'Download all presets from GitHub. Use `--download=<FOLDER>` to download only a subfolder')
            [CompletionResult]::new('-d', '-d', [CompletionResultType]::ParameterName, 'Display documentation')
            [CompletionResult]::new('--doc', '--doc', [CompletionResultType]::ParameterName, 'Display documentation')
            [CompletionResult]::new('--dump-config', '--dump-config', [CompletionResultType]::ParameterName, 'Write the default configuration to the default location. If piped, writes the current configuration to stdout')
            [CompletionResult]::new('-F', '-F ', [CompletionResultType]::ParameterName, 'F')
            [CompletionResult]::new('--test-keys', '--test-keys', [CompletionResultType]::ParameterName, 'test-keys')
            [CompletionResult]::new('--last-key', '--last-key', [CompletionResultType]::ParameterName, 'Print the last key pressed in the last `mm` run')
            [CompletionResult]::new('--no-read', '--no-read', [CompletionResultType]::ParameterName, 'Force the default command to run')
            [CompletionResult]::new('-q', '-q', [CompletionResultType]::ParameterName, 'Reduce the verbosity level')
            [CompletionResult]::new('-v', '-v', [CompletionResultType]::ParameterName, 'Increase the verbosity level')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
    })

    $completions.Where{ $_.CompletionText -like "$wordToComplete*" } |
        Sort-Object -Property ListItemText
}
