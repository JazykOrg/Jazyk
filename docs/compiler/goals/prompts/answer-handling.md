<!-- heading -->
A human answered a standing question on diagnostic {id} ({rule}, {severity}).

Finding: {finding}

<!-- question -->
Question: {question}

<!-- answer -->
The human's answer: "{answer}"

<!-- subjects -->
Subjects:

<!-- subject-quoted -->
- {id}: {statement} (quoted from {doc}#{section}: "{quote}")

<!-- subject-derived -->
- {id}: {statement} ({provenance})

<!-- subject-criterion -->
- {id}: acceptance criterion {criterion}

<!-- subject-entity -->
- {id} ({name}): {definition}

<!-- subject -->
- {id}

<!-- act -->
Treat the human's answer as instructions, never as literal replacement document text. For a ratification proposal, do not automatically accept its insert edit. If asked to link existing text, read the criterion and its owning requirement, find the passage supporting the whole observation, and call update_criterion with id, section, and verbatim quote. Keep the criterion statement and document prose intact unless the human asks to revise them. Handle each member of a combined proposal separately. If no passage supports it, explain the missing detail and refine the question rather than adding redundant prose or quoting an unrelated passage. Act on the answer with the tools: revise_requirement moves a requirement's prose and graph form together, update_requirement and update_entity record decisions, update_diagnostic refines the question when the answer leaves it open. Scope: change only what the answer names, with the smallest edit that satisfies it. Never rewrite sentences the answer does not mention, and never fix unrelated problems in passing; they have their own findings. Linking a pending criterion resolves its ratification automatically; never call resolve_diagnostic on ratification-pending. For other findings, when settled, call resolve_diagnostic with id `{id}` and a reason. Finish with a one-line summary.
