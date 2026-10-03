# Jazyk Cloud plan moved

The active plan and cloud specification live in the private
[JazykOrg/jazyk-cloud repository](https://github.com/JazykOrg/jazyk-cloud), locally
`~/dev/jazyk-cloud`.

- [Cloud plan](https://github.com/JazykOrg/jazyk-cloud/blob/master/plans/cloud/README.md).
- [Repository boundaries](https://github.com/JazykOrg/jazyk-cloud/blob/master/plans/cloud/repositories.md).
- [Independent specification and server layout](https://github.com/JazykOrg/jazyk-cloud/blob/master/plans/cloud/projects.md).

Portable compiler, workspace, delivery, verification, and ACP/LSP contracts remain
in Jazyk. The private service depends on the compiler library. Jazyk does not
depend on the private service to build or run locally.

The [portable delivery proposal](../delivery/README.md) stays in this repository.
This relocation does not change Jazyk's current visibility or remove earlier plans
from its Git history.
