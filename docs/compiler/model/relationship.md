# Relationship

A relationship is a derived node: a typed edge between two [entities](./entity.md). It is
never written directly, and there is no write tool for it. On every commit the store
recomputes relationships from [requirement](./requirement.md) `edges`. See
[derived data](../graph.md#derived-data).

An edge with no requirements cannot exist. Every relationship carries provenance for
free: its `requirements` are exactly the statements that tie the pair together.

## Fields

- The map key is the id: `rel:<a>~<b>`, the two member entity slugs in lexical order.
  E.g. `rel:catalog~shopping-cart`. Like every node type, the id is not repeated inside
  the record.
- `type`: the strongest type across contributing requirement edges. See
  [promotion](#promotion).
- `members`: the two entity ids.
- `requirements`: the contributing requirement ids. Never empty.

## Types

From strongest to weakest:

generalization → realization → composition → aggregation → association → dependency → reference

- `generalization`: is-a (a Dog is an Animal).
- `realization`: fulfills a contract without inheriting implementation (an ArrayList
  realizes a List).
- `composition`: an owned part (a House is composed of Rooms).
- `aggregation`: a shared part, independent of the whole (a Driver in a Car).
- `association`: a lasting connection, one holds a reference to the other (a Student and
  a Course).
- `dependency`: temporary use (a CreditCard depends on FraudDetection).
- `reference`: the weak default. The entities are tied by a statement, nothing structural
  is claimed yet.

## Promotion

The edge type is the strongest type across all contributing requirement edges. A
requirement edge with no declared `type` contributes `reference`. When a requirement is
deleted or its edges change, the recompute on commit demotes or removes the edge. No
manual cleanup is needed, and no cleanup turn runs for it.
