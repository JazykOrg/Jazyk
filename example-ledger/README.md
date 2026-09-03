# example-ledger

The levels chain's fixture: a bookkeeping backend described in four short documents,
written to cross the children limit twice (twelve concepts at the root, then eleven
under Checkout). See `plans/levels.md` for the chain and `plans/walk.md` for the
pages. The documents are the input; `jazyk-out/` (gitignored) is the built state
after the chain: the tree four levels deep, every fan-out under nine, the cards,
the diagram pages, and the diagrams.

Where to look, from this directory:

- `jazyk-out/docsgen/entities/funds.md`: a grouping's card. Open it in a markdown
  preview and walk: `Sits in` goes up, `Inside` goes down, siblings go sideways.
- `jazyk-out/docsgen/diagrams/component/public.md`: the top level's diagram page.
- `jazyk-out/diagrams/component/public.svg`: the top diagram; its boxes link to cards.
- `docs/checkout.md`: hover `Funds` or `Checkout` with the LSP running
  (`bootstrap/editors/vscode`) to see the card in short; go to type definition opens
  the card.
- `jazyk gui` here, then the graph rail: click Checkout in the tree, then a box in
  the overlaid level, then back.

`jazyk compile` from here rebuilds the state on the configured model; the chain took
about ten sessions on a local 27B model.
