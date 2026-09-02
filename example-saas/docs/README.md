# Trellis

Trellis is the name of the product these documents describe: a project tracking tool for
small teams. A User signs up, creates Projects, adds Tasks, invites Members, and pays for
a Subscription. The documents cover what the User can do, how the software is built, and
how it is run.

## Parts

Three things appear in every description of the product:

- The [User](user.md) is the person who uses the product.
- The [Frontend](frontend.md) is the web application the User works in.
- The [Backend](backend.md) is the set of services behind the Frontend.

The Frontend talks only to the Backend. The Backend reaches the User only by email.
[Ops](ops.md) is the practice that keeps the Frontend and the Backend running.

## Documents

Each page below covers one area of the product.

- [user.md](user.md): who the User is and what the User can do.
- [frontend.md](frontend.md): the web application.
- [backend.md](backend.md): the services behind the Frontend and how they fit together.
  - [api-server.md](api-server.md): the service that handles every request.
  - [database.md](database.md): where every record lives.
  - [queue.md](queue.md): background jobs.
  - [cache.md](cache.md): the read cache.
- [identity.md](identity.md): the Account, Organization, Member, Session, and ApiKey
  classes, with the signup and invitation flows.
- [collaboration.md](collaboration.md): the Project, Task, Comment, Attachment,
  Notification, and AuditEntry classes, with the project creation flow.
- [billing.md](billing.md): the Plan, Subscription, and Invoice classes, with the
  invoice payment flow.
- [ops.md](ops.md): the four practices that keep the product running.
  - [monitoring.md](monitoring.md)
  - [deployment.md](deployment.md)
  - [backups.md](backups.md)
  - [support.md](support.md)
