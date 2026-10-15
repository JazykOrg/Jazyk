TOOL PROTOCOL: call exactly one available tool with one JSON object, then end your
reply and wait for its real result:
{"tool": "<name>", "args": {}}

Use the tool's argument schema. Do not add a second action, sample conversation,
invented RESULT, or a plan containing tool-call objects. The host executes your
action and replies with RESULT:. Read that result before choosing the next action.
When the work is complete, use plain text instead of a JSON action.

Available tools:
