# Turn-Based Refactor

The current compilation is not working as expected. We need to refactor the strategy and try another approach.

Instead of multiple step compilation transforming docs into reqs, let's take a different approach.

At first, we are going to focus on how an AI will read in context for a particular entity, requirement or doc section.
Once we have that context, we can allow an AI to progressively iterate on the content with the right context and
progressively refine the content into reqs.

We will create a harness that will allow reading in context and perform actions via tools. This harness will be
the core part of the compilation process.

# Context

When an LLM performs a specific task, it needs to be given just the right amount of context, and a way to quickly
load further context in the correct direction and correct sizing.

## Initial scope

The context scope will be fetched given a set of specific target items, their focus and limits on the size.

### Target

The context focus will narrow down the item that requires context, this may be a:

1. Documentation section
2. Entity
3. Requirement

### Focus

In addition to the target items, a focus will narrow down which relationships to include, whether to include the
relationship itself or the related item as well and its relationships.

The context load arguments will indicate how many hops of relationships to include per each relationship type.
Relationship types are:
- Documentation parent
- Documentation mention
- Entity requirement

E.g. A focus on an entity `A` with:
- `Documentation parent` hop size of 2
- `Documentation mention` hop size of 1
- and `Entity requirement` hop size of 2

This will load the following:
- Doc B mentions entity A (first hop doc mention)
- Doc B has a parent entity B' which has a parent of B'' (2-hops of parent docs)
- Requirement D links entity `A` with entity `E` (first hop requirement)
- Requirement F links entity `E` with entity `G` (second hop requirement)


### Size

The context will also be limited by the total size to ensure the receiving LLM does not get overloaded. As the
information graph is traversed to load the target and its focus, the size will be tracked and once reached, will
stop loading further context.

## Expansion

Along with the initial scope, the context can be further expanded via additional tool calls. The omitted
relationships will be indicated with hints on how to load them.

# Compilation

The entire compilation process needs to be reworked, there are two cases to handle, initial compilation and
recompilation. The recompilation is likely the easier path, as you are given the existing docs, entities,
relationships, and given the docs diff to compile. The harness is able to modify the relationships, entities
to update it based on the diff.

For initial state, this will likely have to be done iteratively, but parallelized when possible. Start at the
root doc to create the initial entities and relationships and then dig down to sub-docs to expand. Ideally
both compilaiton and recompilation is the SAME process.

While I described the MCP context above for what the LLM sees, there is a second part to this which is the
tools it can call. These are grouped by the Context Expansion that allows you to read more of the graph,
and the second set of tools is to allow you to modify the graph. While the expansion will be used by external
LLMs that eventually do code generation, or code assistance etc.., the graph modification tools will be only
as part of our internal compilation harness. I am imagining creating updating deleting entities, requirements,
etc.
