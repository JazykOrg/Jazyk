---
requirement: req:main-js-2
hash: e32d8eb64f849321
---

# Statement

The sort utility shall sort input similarly to the `sort` CLI command.

# Quote

> This is a simple sorting algorithm CLI utility similar to the `sort` CLI command.

# Implementing files

- main.js

# Steps to confirm

1. Read `main.js` and confirm it reads lines from STDIN, sorts them, and prints the sorted lines to STDOUT, the same overall contract as the POSIX `sort` command.
2. Run `printf 'b\na\nc\n' | node main.js` and compare with `printf 'b\na\nc\n' | sort`: both should print `a`, `b`, `c` on separate lines.
3. Confirm the default order is lexicographic (string comparison), matching `sort`'s default collation behavior for simple ASCII input.
4. Note accepted deviations: unlike `sort`, this utility strips whitespace and drops empty lines (its own documented behavior). "Similarly" does not require identical behavior, only the same core function: line-based lexicographic sorting of STDIN to STDOUT.

# Verdict contract

Reply PASS if the utility performs line-based lexicographic sorting of STDIN to STDOUT like the `sort` command, with deviations limited to its own documented behaviors (whitespace stripping, empty-line skipping, `-r`). Reply FAIL otherwise. Include reasoning citing the observed outputs.
