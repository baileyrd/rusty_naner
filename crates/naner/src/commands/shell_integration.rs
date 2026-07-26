//! Command: `naner shell-integration <pwsh|bash|zsh|fish>`
//! Generates terminal OSC 133 shell integration scripts for prompt marking
//! and command lifecycle event notification (compatible with rusty_term / l13 / MCP).

pub fn execute(args: &[String]) -> i32 {
    let shell = args.first().map(|s| s.to_lowercase()).unwrap_or_else(|| "pwsh".to_string());

    match shell.as_str() {
        "pwsh" | "powershell" => {
            println!(r#"# Naner / rusty_term OSC 133 PowerShell Shell Integration
function prompt {{
    $lastExit = $?
    $exitCode = if ($lastExit) {{ 0 }} else {{ 1 }}
    # OSC 133 D: Command finished
    Write-Host -NoNewline "$([char]27)]133;D;$exitCode$([char]7)"
    # OSC 133 A: Prompt start
    Write-Host -NoNewline "$([char]27)]133;A$([char]7)"
    "PS $($executionContext.SessionState.Path.CurrentLocation)> "
    # OSC 133 B: Command start
    Write-Host -NoNewline "$([char]27)]133;B$([char]7)"
}}
"#);
            0
        }
        "bash" => {
            println!(r#"# Naner / rusty_term OSC 133 Bash Shell Integration
__naner_prompt_command() {{
    local exit_code="$?"
    printf "\033]133;D;%d\007" "$exit_code"
    printf "\033]133;A\007"
}}
PROMPT_COMMAND="__naner_prompt_command;$PROMPT_COMMAND"
PS1='\[\033]133;B\007\]\u@\h:\w\$ '
"#);
            0
        }
        "zsh" => {
            print!("{}", r#"# Naner / rusty_term OSC 133 Zsh Shell Integration
precmd() {
    local exit_code="$?"
    printf "\033]133;D;%d\007" "$exit_code"
    printf "\033]133;A\007"
}
preexec() {
    printf "\033]133;C\007"
}
PS1=$'%{\e]133;B\a%}'"$PS1"
"#);
            0
        }
        "fish" => {
            println!(r#"# Naner / rusty_term OSC 133 Fish Shell Integration
function __naner_postexec --on-event fish_postexec
    printf "\033]133;D;%d\007" $status
end
function __naner_prompt_start --on-event fish_prompt
    printf "\033]133;A\007"
end
"#);
            0
        }
        other => {
            eprintln!("Unknown shell '{other}'. Supported: pwsh, bash, zsh, fish");
            1
        }
    }
}
