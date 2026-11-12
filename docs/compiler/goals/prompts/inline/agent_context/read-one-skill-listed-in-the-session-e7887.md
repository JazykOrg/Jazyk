---
workflow: >-
  Before an agent starts work, Jazyk loads the project instructions, optional skills,
  and connections to other tools. These messages describe how that extra context is
  loaded and which connection settings are allowed.
used_by:
  - "bootstrap/src/agent_context.rs"
---
Read one skill listed in the session's SKILLS block: its instructions and the files in its folder. Read a skill before following it.