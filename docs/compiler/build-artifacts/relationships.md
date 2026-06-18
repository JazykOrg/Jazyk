# Relationships

Any section in documentation can be related to another section.

## Types

Several relationship types are supported and their relative strength from strongest to weakest is:

Generalization → Realization → Composition → Aggregation → Association → Dependency

### Inheritance family

#### Generalization

Generalization is the classic "is-a" (e.g. A Dog is an Animal), so you inherit the parent's structure and behaviour.
This is typically implemented as a class inheritance.

#### Realization

A promise to fulfill a contract without inheriting any implementation. (e.g. ArrayList realizes/implements
List interface)

### Whole-part family

#### Composition

You are part of another and owned by that another. (e.g. a House is composed of a Room)

#### Aggregation

You are part of another and maye be shared by others. (e.g. a Driver is part of a Car, but Driver can exist without it
or be part of multiple Cars)

### Link family

#### Association

Connection that tends to persist between two objects. One object holds a reference to the other. (e.g.
a student is associated with a course)

#### Dependency

Connection that tends to be temporary and/or optional. An object is given a temporary reference via method parameter
(e.g. CreditCard depends on FraudDetection)

## Cardinality

Some relationship types support cardinality constraints such as `0..*`, `1` or `1..*`.
Cardinality is applied on both sides of the relationship.
