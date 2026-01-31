#!/usr/bin/env bash

# Bash completion script for lado - Git diff viewer

_lado_completions() {
    local cur prev
    COMPREPLY=()
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"

    # Handle --completions option
    if [[ "$prev" == "--completions" ]]; then
        COMPREPLY=($(compgen -W "bash zsh fish powershell elvish" -- "$cur"))
        return 0
    fi

    # Options
    if [[ "$cur" == -* ]]; then
        COMPREPLY=($(compgen -W "--completions --help --version" -- "$cur"))
        return 0
    fi

    # Branches and PRs
    local branches prs
    branches=$(git branch --format='%(refname:short)' 2>/dev/null)

    # Include remote branches
    branches="$branches $(git branch -r --format='%(refname:short)' 2>/dev/null | sed 's/origin\///')"

    # Include PRs if gh is available
    if command -v gh &>/dev/null; then
        prs=$(gh pr list --limit 20 --json number --jq '.[].number' 2>/dev/null | sed 's/^/#/')
    fi

    COMPREPLY=($(compgen -W "$branches $prs" -- "$cur"))
}

complete -F _lado_completions lado
