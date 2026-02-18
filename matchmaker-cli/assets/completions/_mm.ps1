
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
            [CompletionResult]::new('--dump-config', '--dump-config', [CompletionResultType]::ParameterName, 'dump-config')
            [CompletionResult]::new('-F', '-F ', [CompletionResultType]::ParameterName, 'F')
            [CompletionResult]::new('--test-keys', '--test-keys', [CompletionResultType]::ParameterName, 'test-keys')
            [CompletionResult]::new('--no-read', '--no-read', [CompletionResultType]::ParameterName, 'Force the default command to run')
            [CompletionResult]::new('-q', '-q', [CompletionResultType]::ParameterName, 'Reduce the verbosity level')
            [CompletionResult]::new('-v', '-v', [CompletionResultType]::ParameterName, 'Increase the verbosity level')
            [CompletionResult]::new('--options', '--options', [CompletionResultType]::ParameterName, 'Display options doc')
            [CompletionResult]::new('--binds', '--binds', [CompletionResultType]::ParameterName, 'binds')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
    })

    $completions.Where{ $_.CompletionText -like "$wordToComplete*" } |
        Sort-Object -Property ListItemText
}
