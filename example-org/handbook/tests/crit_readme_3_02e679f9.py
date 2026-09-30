#!/usr/bin/env python3
# req:readme-11  crit:readme-3  statementHash: 47102a4b9e2663a2
# test name: crit_readme_3_02e679f9
# Criterion crit:readme-3 (positive): The `handbook/` folder holds a markdown page
# for each source document in the folder.
#
# The source documents (docs/README.md#/ridgeline-outfitters/the-handbook) are:
#   README.md, departments.md, hiring.md, expenses.md, policies.md
# The handbook must deliver one markdown page per source document under handbook/.
import os
import sys

SOURCE_DOCS = ["README.md", "departments.md", "hiring.md", "expenses.md", "policies.md"]

def crit_readme_3_02e679f9():
    missing = [d for d in SOURCE_DOCS
               if not os.path.isfile(d)]
    if missing:
        print("FAIL: handbook/ is missing a page for: " + ", ".join(missing))
        return 1
    print("PASS: handbook/ holds a markdown page for each source document")
    return 0

if __name__ == "__main__":
    sys.exit(crit_readme_3_02e679f9())
