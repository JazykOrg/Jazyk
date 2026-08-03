# Main.ts

This is a simple sorting algorithm CLI utility similar to the `sort` CLI command.

## Overview

This is a script written in javascript.

## CLI args

When invoked, `sort` allows the following CLI args:
- `-r` reverses sorting order to descending

## Execution

This is pseudo code:

```Pseudo
Read CLI arguments, keep track of:
- Reverse order with `-r`
Initialize array of strings called lines
Read in from STDIN line by line:
    For current line, strip out whitespace before and after
    If stripped line is empty string, continue to next line
    Add stripped line to lines
Sort lines ascending. Or descending if reverse order
Print all sorted lines delimited by newline
```