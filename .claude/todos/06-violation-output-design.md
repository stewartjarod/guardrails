# Design and Show Violation Output

## Problem
The README shows BadCard/GoodCard source code but never shows what the tool output looks like. People evaluate tools by their output — the terminal experience IS the demo.

## Why This Matters
A screenshot of beautiful, clear violation output in the README sells the tool faster than any feature list. It answers "what will I actually see?" before they install.

## What to Do
- [x] Design the pretty-print violation format:
  ```
    src/BadCard.tsx
      12:24  error    bg-white has no dark: variant          enforce-dark-mode
                      Use bg-background instead
      15:8   warning  text-gray-900 is a raw color class     use-theme-tokens
                      Use text-foreground instead

    28 problems (20 errors, 8 warnings)
  ```
- [x] Use colors: red for errors, yellow for warnings, dim for suggestions, bold for file paths
- [x] Show source line context with an underline/caret pointing to the violation column
- [x] Design the compact format (one line per violation, for piping/grep)
- [x] Design the JSON format (for programmatic consumption)
- [x] Design the GitHub Actions format (::error/::warning annotations)
- [x] Add a summary line at the end (X problems, Y errors, Z warnings)
- [ ] Take a screenshot/recording of the output and add it to the README
- [ ] Consider an SVG terminal render for the README (like asciinema or vhs)

## Success Criteria
The README contains a visual of the tool output that makes someone think "I want that in my terminal." All four output formats are implemented and documented.
