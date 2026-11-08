# example-ledger

The levels chain's fixture: a bookkeeping backend described in four short documents,
written to cross the children limit twice (twelve concepts at the root, then eleven
under Checkout). See [levels](../docs/compiler/concepts/levels.md) for the chain.
The documents are the input; `.jazyk/` (gitignored) is the built state after the
chain: the tree four levels deep, every fan-out under nine, the cards,
the diagram pages, and the diagrams.

Where to look, from this directory:

- `.jazyk/docsgen/entities/funds.md`: a grouping's card. Open it in a markdown
  preview and walk: `Sits in` goes up, `Inside` goes down, siblings go sideways.
- `.jazyk/docsgen/diagrams/component/public.md`: the top level's diagram page.
- `.jazyk/diagrams/component/public.svg`: the top diagram; its boxes link to cards.
- `docs/checkout.md`: hover `Funds` or `Checkout` with the LSP running
  (`bootstrap/editors/vscode`) to see the card in short; go to type definition opens
  the card.
- `jazyk gui` here, then the graph rail: click Checkout in the tree, then a box in
  the overlaid level, then back. The explorer's `generated` section opens the same
  cards rendered, diagrams inline and clickable.
- The owner's side of a grouping: `jazyk answer g:ratify:ent:funds` lists the
  proposal's options; `--option 0` accepts it (the sentence lands in
  `docs/checkout.md` and the next build flips Funds to quoted provenance);
  `--option 1` retracts it (the members return to Checkout).

`jazyk compile` from here rebuilds the state on the configured model; the chain took
about ten sessions on a local 27B model.
