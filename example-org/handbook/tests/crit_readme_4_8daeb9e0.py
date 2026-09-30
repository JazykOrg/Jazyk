#!/usr/bin/env python3
# req:readme-12  crit:readme-4  statementHash: d10b9a1a5c7a1df2
# test name: crit_readme_4_8daeb9e0
# Criterion crit:readme-4 (positive): The `handbook/` folder includes the pages
# `README.md`, `departments.md`, `hiring.md`, `expenses.md`, and `policies.md`.
#
# The employee handbook (docs/README.md#/ridgeline-outfitters/the-handbook) must
# deliver these five pages under handbook/.
import os
import sys

PAGES = ["README.md", "departments.md", "hiring.md", "expenses.md", "policies.md"]

def crit_readme_4_8daeb9e0():
    missing = [p for p in PAGES
               if not os.path.isfile(p)]
    if missing:
        print("FAIL: handbook/ is missing page(s): " + ", ".join(missing))
        return 1
    print("PASS: handbook/ includes README.md, departments.md, hiring.md, expenses.md, and policies.md")
    return 0

if __name__ == "__main__":
    sys.exit(crit_readme_4_8daeb9e0())
