# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Baseline is a code quality enforcement tool that lints React/TypeScript projects for Tailwind CSS best practices and semantic theming. Rules are implemented in Rust and configured via TOML.

## Build Commands

- `cargo check` — type-check without building
- `cargo build` — compile the library
- `cargo test` — run all tests

## Architecture

**Rule system**: Plugin-based architecture where each rule implements the `Rule` trait (`id()`, `severity()`, `file_glob()`, `check_file()`). Rules receive a `ScanContext` (file path + content) and return `Vec<Violation>`.

**Core types** (from `crate::config` and `crate::rules`):
- `RuleConfig` — parsed TOML rule configuration (`src/config.rs`)
- `Severity` — error/warning levels (`src/config.rs`)
- `ScanContext` — file being scanned (path + content) (`src/rules/mod.rs`)
- `Violation` — detected issue (rule_id, severity, file, line, column, message, suggest, source_line) (`src/rules/mod.rs`)
- `RuleBuildError` — construction errors (e.g., `InvalidRegex`) (`src/rules/mod.rs`)

**Two rule implementations exist**:
- `TailwindDarkModeRule` (`src/rules/tailwind_dark_mode.rs`) — ensures hardcoded Tailwind color classes have `dark:` variants. Automatically allows shadcn semantic tokens (`bg-background`, `text-foreground`, etc.) and special values (`transparent`, `current`).
- `TailwindThemeTokensRule` (`src/rules/tailwind_theme_tokens.rs`) — bans raw Tailwind color classes and suggests shadcn semantic token replacements. Ships with ~130+ default mappings in `default_token_map()`.

**Class extraction**: Both rules detect classes from `className=`, `class=`, and utility function calls (`cn()`, `clsx()`, `classNames()`, `cva()`, `twMerge()`). They skip `dark:`, `hover:`, and `focus:` prefixed classes.

## Configuration

`examples/baseline.toml` is the sample config. `examples/baseline.example.toml` documents all supported rule types:
- `banned-import`, `banned-pattern`, `required-pattern`, `banned-dependency`, `file-presence`, `ratchet`, `tailwind-dark-mode`, `tailwind-theme-tokens`

Each `[[rule]]` has: `id`, `type`, `severity`, `glob`, `message`, `suggest`, plus type-specific fields (`allowed_classes`, `token_map`, `packages`, `pattern`, `max_count`, etc.).

## Example Files

`examples/BadCard.tsx` and `examples/GoodCard.tsx` demonstrate anti-patterns vs. best practices for shadcn/ui theming. Use these as reference when writing or modifying rules.
