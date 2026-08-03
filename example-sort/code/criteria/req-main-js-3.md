---
requirement: req:main-js-3
hash: 1abb960e6eac267c
---

# Statement

The sort utility shall be a script.

# Quote

> This is a script written in javascript.

# Implementing files

- main.js

# Steps to confirm

1. Read `main.js` and confirm it is a single interpretable source file, not a compiled artifact and not a project requiring a build step.
2. Confirm it carries a `#!/usr/bin/env node` shebang line so it can be executed directly as a script.
3. Run `printf 'b\na\n' | node main.js` and confirm it executes as-is, with no compilation, bundling, or install step.

# Verdict contract

Reply PASS if `main.js` is directly interpretable source executed by node with no build step and carries a shebang. Reply FAIL otherwise. Include reasoning citing the file contents and the command output.
