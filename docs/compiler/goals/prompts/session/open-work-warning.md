---
purpose: Explain accepted overlapping writes and later commit conflicts.
consumer: Staging and read replies for persisted open work.
---
Staged work overlaps another open changeset. This warning does not reject your
change or require an override. Inspect its mutations with `read_open_work`,
continue, wait, or coordinate outside Jazyk. If that work commits first, your
commit reply will report merge conflicts with its committed changes. Disjoint
fields merge; your later commit wins overlapping fields and reports what it
overwrites, subject to integrity and completion gates.
