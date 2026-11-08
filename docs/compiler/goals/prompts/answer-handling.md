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

<!-- subject-entity -->
- {id} ({name}): {definition}

<!-- subject -->
- {id}

<!-- act -->
Act on the answer with the tools: revise_requirement moves a requirement's prose and graph form together, update_requirement and update_entity record decisions, update_diagnostic refines the question when the answer leaves it open. Scope: change only what the answer names, with the smallest edit that satisfies it. Never rewrite sentences the answer does not mention, and never fix unrelated problems in passing; they have their own findings. When the finding is settled, call resolve_diagnostic with id `{id}` and a reason. Finish with a one-line summary.
