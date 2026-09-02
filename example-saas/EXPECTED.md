# SaaS expected outcomes (hand-labeled, not part of the docs glob)

Counts are indicative, not graded: a richer graph is acceptable when every extra
requirement is independently testable and every extra entity has statements directly
about it. The graded part is the level shape, the level views, the drill-down chain,
and the traps. Stereotypes are the model's judgment; the labels below are the expected
kind, not a required spelling.

This fixture is written to the owner's picture: three boxes at the top (User, Frontend,
Backend), the Backend opening into its four components, the API Server opening into
areas the documents only suggest by headings, and the areas opening into the classes
behind the Database tables. The corpus states the leaves and the first two levels; the
levels in between are the model's to mint.

## Entities per level

### The root (`scope:public`), stated

Four parentless entities, all stated by README.md and their own pages:

- User: `actor`. No parent.
- Frontend: `component` or `application` ("a single-page web application").
- Backend: `system` ("the set of services behind the Frontend"). Alias `backend
  services` (trap 2).
- Ops: stereotype the model's choice (`system`, `process`, or none). Its page states
  "Ops has four parts:".

Borderline: Trellis, the product name. README.md names it as the name of the product,
never as a subject that contains anything. Acceptable unminted, or as a parentless
entity with no children. Wrong: Trellis as a root containing Frontend and Backend, which
no sentence states, and which turns the three-box top diagram into a two-box one.

The root is at four, under the soft threshold of nine. No `fan-out` goal derives on
`scope:public` and no root grouping is minted (trap 3).

### Backend, stated

backend.md's "It is made up of four components:" list gives four `composition` edges and
four children with `parent: ent:backend`:

- API Server: `component` or `service` ("a Go program exposing a JSON API").
- Database: `component` ("the PostgreSQL 16 instance").
- Queue: `component`.
- Cache: `component` ("the Redis instance").

Four children, no fan-out goal. Wrong: any of the four under Ops, or a grouping of two of
them.

### Ops, stated and flat

ops.md's "Ops has four parts:" list gives four children with `parent: ent:ops`:
Monitoring, Deployment, Backups, Support. Each has its own page. Four children, no
fan-out goal, and no grouping may be minted among them (trap 6). Their stereotype is
the model's choice; the level view's kind follows it.

### API Server, stated leaves, minted areas

api-server.md states twenty-one children of the API Server across three headed lists,
each item a `composition` from the API Server and each child's `parent` the API Server:

- Under "## Data model", fourteen classes, defined in identity.md, collaboration.md, and
  billing.md: Account, Organization, Member, Session, ApiKey, Project, Task, Comment,
  Attachment, Notification, AuditEntry, Plan, Subscription, Invoice. No stereotype or
  `class`. Attributes from "An X has ..." sentences: Account (`email address`, `display
  name`, `password hash`, `status`), Organization (`name`, `slug`), Member (`role`),
  Session (`token`, `account`, `expiry`), ApiKey (`name`, `hashed secret`, `account`,
  `expiry`), Project (`name`, `description`, `organization`), Task (`title`, `status`,
  `assignee`, `due date`), Comment (`body`, `author`, `task`), Attachment (`file name`,
  `size`, `content type`, `task`), Notification (`recipient`, `event text`, `link`,
  `read`), AuditEntry (`actor`, `action`, `target`, `organization`, `timestamp`), Plan
  (`name`, `monthly price`, `member limit`, `project limit`), Subscription
  (`organization`, `plan`, `billing period start`, `status`), Invoice (`subscription`,
  `period`, `amount`, `status`, `payment date`). Keeping some as statement detail is
  acceptable.
- Under "## Request handling", four handlers: Auth Handler, Organization Handler,
  Project Handler, Billing Handler. Stereotype `module` or none.
- Under "## Rendering", three renderers: JSON Serializer, Email Templates, CSV
  Exporter. Stereotype `module` or none.

Twenty-one is over the hard threshold of fifteen: a mandatory `fan-out`
`abstract-entity` goal derives on `ent:api-server` (trap 4). A good run mints three
groupings, named from the headings the documents already use for the areas:

- a data model grouping (Data Model, or Data model; the slug follows the minted name),
  fourteen members, `stereotype: module` or none;
- a request handling grouping (Request Handling, or Handlers), four members;
- a rendering grouping (Rendering, or Presentation), three members.

Each grouping has `provenance: derived` with `from` naming exactly its members and a
reasoning, a one-sentence `definition`, `parent: ent:api-server`, no mentions, and a
ratification proposal toward api-server.md. After the session the API Server has three
children. The documents never state a "Data model", "Request handling", or "Rendering"
entity (trap 8): a grouping with quote provenance anchored on a heading is wrong.

### The data model grouping, minted, over soft

Fourteen children is over the soft threshold of nine and under the hard one: an optional
`fan-out` goal derives on the data model grouping. The documents split the classes three
ways by page, which is the naming hint:

- Identity (identity.md): Account, Organization, Member, Session, ApiKey. Five members.
- Collaboration (collaboration.md): Project, Task, Comment, Attachment, Notification,
  AuditEntry. Six members.
- Billing (billing.md): Plan, Subscription, Invoice. Three members.

A good run mints these three, each with `parent` the data model grouping, and the data
model level drops to three. A weak run leaves the fourteen flat under an open optional
goal, or fails the goal: `optional advised` in the verdict, never blocking convergence,
but the level shape line shows one node over soft. A grouping that moves Notification
and AuditEntry into a fourth grouping of their own (Activity, or Events) is acceptable
with a reason; a grouping that mixes documents without a reason is weaker.

The request handling and rendering groupings stay at four and three: no further goal.

## Requirements and edges

At least ~120 requirements across the sixteen documents, one per obligation. No
obligation the documents state may be missing. Edges the prose states, `a` acting on
`b`:

- Backend → API Server, Database, Queue, Cache: `composition`. Ops → Monitoring,
  Deployment, Backups, Support: `composition`. API Server → each of its twenty-one
  children: `composition`.
- User → Frontend, `dependency` ("the web application the User works in"); Frontend →
  Backend, `dependency` ("The Frontend talks only to the Backend"); Frontend → API
  Server, `dependency`.
- API Server → Database, `dependency`; API Server → Cache, `dependency`; API Server →
  Queue, `dependency`; Queue → Database, `dependency` ("stores every job in the Database
  table `jobs`").
- User → Account, `association`, cardinality `1`; Account → Session, `aggregation`
  `0..*`; Account → ApiKey, `aggregation` `0..*` (at most 10); Member → Account and
  Member → Organization, `association`; Organization → Member, `aggregation` `1..*`.
- Organization → Project, `composition`; Project → Task, `composition` (at most 5000);
  Task → Comment and Task → Attachment, `composition`; Task → Member, `association`
  (the assignee); Comment → Member, `association` (the author); Notification → Account,
  `association`; AuditEntry → Organization, `association`.
- Organization → Subscription, `composition` `1`; Subscription → Plan, `association`;
  Invoice → Subscription, `association`; Support → Invoice, `dependency` (refunds).
- Monitoring → API Server, Queue, Database, `dependency`; Deployment → Frontend,
  Database, API Server, `dependency`; Backups → Database, `dependency`; Support → User,
  `dependency`.

Multi-entity requirements left without edges open optional `declare-edges` goals; they
count as `optional advised`.

## State machines

Transitions the prose states, one machine per subject:

- `sm:account`: `unverified → verified` (the User opens the verification link);
  `verified → suspended` (10 failed logins in an hour); `suspended → verified` (password
  reset). Initial `unverified`.
- `sm:task`: `open → in progress` (the assignee starts the Task); `in progress → done`
  (the assignee completes it); `done → open` (a Member reopens it). Initial `open`. No
  dead end.
- `sm:invoice`: `open → paid` (the charge succeeds); `open → overdue` (the charge fails,
  a `failure-mode`); `overdue → paid` (the User pays). Initial `open`. `paid` is the
  final state.
- `sm:subscription`: `active → cancelled` (an owner cancels, or an Invoice stays overdue
  for 14 days). Initial `active`.
- Borderline: a machine on the Queue's job (`queued`, `running`, `done`, `failed`).
  queue.md says "A job is in one of the states", so a Job entity under the Queue with
  its machine is acceptable; keeping the states as requirement detail on the Queue is
  acceptable too.

`dead-end-state` (info) names `paid`, `cancelled`, and `suspended` when its exit is not
extracted. Not graded.

## Fan-out goals

| target | children | threshold | goal | expected outcome |
| --- | --- | --- | --- | --- |
| `scope:public` | 4 | under soft | none | stays four |
| `ent:backend` | 4 | under soft | none | stays four |
| `ent:ops` | 4 | under soft | none | stays four, never grouped |
| `ent:api-server` | 21 | over hard (15) | mandatory `abstract-entity`, `fan-out` | three groupings from the headings; the session must land at or under 15 or `converged` is blocked |
| the data model grouping | 14 | over soft (9) | optional `abstract-entity`, `fan-out` | good run: Identity, Collaboration, Billing; weak run: left open or failed, `optional advised` |
| the request handling grouping | 4 | under soft | none | stays four |
| the rendering grouping | 3 | under soft | none | stays three |

The coupling hints on `ent:api-server` partition by shared requirements: the fourteen
classes share identity.md, collaboration.md, and billing.md requirements among
themselves and with the handlers; the handlers share api-server.md's request handling
section; the renderers share its rendering section. The hints may propose the handlers
and the renderers as one cluster (the flows tie the Auth Handler to the Email Templates).
The model splits them with a reason: the documents treat request handling and rendering
as two areas. Accepting a merged handlers-and-renderers grouping with a reason is
acceptable but weaker.

The API Server goal is ready only when every compile goal in its cone is closed: every
class page reconciled, every `rejudge-pair` in the subtree done. The data model goal is
ready only after the API Server goal commits (the grouping does not exist before).

## Level views

Every node with two or more children gets a structural level view, `default: true`,
members the direct children plus every outside entity with a lifted edge into the
level, in document order. The kind is `component` when the node or any child carries a
structural stereotype, `class` otherwise.

| view | node | members |
| --- | --- | --- |
| `view:component/public` | the scope root | User, Frontend, Backend, Ops (User is an actor, so the kind is `component`); title `Public` |
| `view:component/backend` | Backend | API Server, Database, Queue, Cache, plus the outside entities whose edges lift in: Frontend (calls the API Server), Monitoring, Deployment, Backups (touch the Database and the API Server), User (when a step names the User and a component directly) |
| `view:class/ops` or `view:component/ops` | Ops | Monitoring, Deployment, Backups, Support, plus Backend (lifted from their edges into the Database, the Queue, and the API Server), Frontend, User, Invoice's ancestor (Support refunds an Invoice, lifted to Backend) |
| `view:component/api-server` | API Server | the three groupings after the mandatory goal, plus Frontend, Database, Queue, Cache, User (lifted from the flows); before the goal, the twenty-one stated children, over-limit and auto-collapsed. The kind is `component` because the API Server itself carries `component` or `service`; `view:class/api-server` only when the model leaves it unstereotyped |
| `view:class/<data-model-slug>` | the data model grouping | the fourteen classes before the optional goal; Identity, Collaboration, Billing after it, plus the request handling grouping and the rendering grouping (the handlers write the classes and the renderers read them), Database, Cache, User, Support |
| `view:class/<request-handling-slug>` | the request handling grouping | the four handlers plus the data model grouping (or its sub-groupings), the rendering grouping, Frontend, Database, Queue, User |
| `view:class/<rendering-slug>` | the rendering grouping | the three renderers plus Queue, the request handling grouping, Frontend |
| `view:class/identity`, `view:class/collaboration`, `view:class/billing` | the sub-groupings, in a good run | their classes plus the classes and handlers with edges into them |

The scope root's level view is the per-scope view: no separate `view:class/public`
derives beside `view:component/public`. The slugs of the minted groupings follow their
minted names: `data-model` for "Data Model", `request-handling` for "Request Handling",
`rendering` for "Rendering". A different name with the same meaning ("Handlers",
"Presentation") gives a different slug and is acceptable.

## Flow views per level

The four flows are step lists: Signup and Inviting a member in identity.md, Creating a
project in collaboration.md, Paying an invoice in billing.md. Every step is a
`behavior` requirement naming the User, the Frontend, or a Backend component or class.
The failure sentences after each list are `failure-mode` requirements. Per level, the
harness lifts each step's entities to their nearest ancestor in the level, clusters by
the lifted actor and document, and derives a `use-case` and a `sequence` view for every
cluster of two or more.

At the root, every step lifts to User, Frontend, or Backend. The scope root keeps the
unprefixed flow ids (`<actor-slug>-<doc-stem>`); only the levels below prefix the
node's slug:

- `view:usecase/user-identity` and `view:sequence/user-identity`, title
  `User: Identity`: the signup and invitation steps, participants User, Frontend,
  Backend.
- `view:usecase/user-collaboration` and its sequence twin, title
  `User: Collaboration`.
- `view:usecase/user-billing` and its sequence twin, title `User: Billing`. The
  first three billing steps name no User; they key on the lifted first entity (Backend)
  and form `backend-billing` when two or more, which is mechanism-faithful.

Inside Backend, the same steps lift to API Server, Database, Queue, or Cache; the User
and the Frontend stay outside participants:

- `view:usecase/backend-<actor>-identity`, `view:usecase/backend-<actor>-collaboration`,
  `view:usecase/backend-<actor>-billing` and their sequence twins, where `<actor>` is
  `user` when the harness keeps the outside actor as the key and `api-server` when it
  keys on the lifted first entity. Either is acceptable; the sequence twin shows
  messages among API Server, Database, Queue, and Cache.

Inside the API Server (after the mandatory goal), the steps lift to the request handling
grouping and the rendering grouping, with the data model grouping as the written
records: one cluster per flow document again, ids prefixed `api-server-`. Inside the data
model grouping, the steps lift to Identity, Collaboration, and Billing (a good run) or
to the classes themselves.

Ops derives no flow view: no step list names Monitoring, Deployment, Backups, or
Support. Their obligations are single behaviors and quality bounds, and each lone
behavior is `unplaced-behavior` (info), `optional advised`.

## Drill-down

The chain the owner described, as links from a member's element to the level view
below it, in the `.puml` as `[[../<kind>/<slug>.svg]]` and in the `.svg` as anchors:

1. `view:component/public`: Backend links to `view:component/backend`; Ops links to its
   level view; User and Frontend carry no link (no children).
2. `view:component/backend`: API Server links to `view:component/api-server`;
   Database, Queue, and Cache carry no link.
3. `view:component/api-server`: each of the three groupings links to its level view.
4. `view:class/<data-model-slug>`: in a good run, Identity, Collaboration, and Billing
   each link to their level view; in a weak run the fourteen classes carry no link.
5. `view:class/identity` and its siblings: the leaf classes, no links.

`get_view` on each of these carries a `children` list with one entry per linked member.
Docsgen nests one page per level with a breadcrumb up. The viewer prints the tree with
each node's view ids; `jazyk status` shows nodes per depth (a good run: 4, 8, 3, 10,
14 by depth from the root: the four roots; Backend's four and Ops's four; the three
groupings under the API Server; Identity, Collaboration, Billing beside the four
handlers and three renderers; the fourteen classes) and a fan-out histogram with no
node over the hard threshold.

## Rendered diagrams

A converged compile produces, each `.puml` with its `.svg` beside it:

```
jazyk-out/diagrams/component/public.puml               .svg
jazyk-out/diagrams/component/backend.puml              .svg
jazyk-out/diagrams/class/ops.puml                      .svg   (or component/)
jazyk-out/diagrams/component/api-server.puml           .svg
jazyk-out/diagrams/class/<data-model-slug>.puml        .svg
jazyk-out/diagrams/class/<request-handling-slug>.puml  .svg
jazyk-out/diagrams/class/<rendering-slug>.puml         .svg
jazyk-out/diagrams/class/identity.puml                 .svg   (good run)
jazyk-out/diagrams/class/collaboration.puml            .svg   (good run)
jazyk-out/diagrams/class/billing.puml                  .svg   (good run)
jazyk-out/diagrams/usecase/user-identity.puml          .svg   and sequence/
jazyk-out/diagrams/usecase/user-collaboration.puml     .svg   and sequence/
jazyk-out/diagrams/usecase/user-billing.puml           .svg   and sequence/
jazyk-out/diagrams/usecase/backend-*.puml              .svg   and sequence/
jazyk-out/diagrams/usecase/api-server-*.puml           .svg   and sequence/
jazyk-out/diagrams/state/account.puml                  .svg
jazyk-out/diagrams/state/task.puml                     .svg
jazyk-out/diagrams/state/invoice.puml                  .svg
jazyk-out/diagrams/state/subscription.puml             .svg
```

What the pictures show:

- `component/public`: `actor User`, `component Frontend`, `component Backend` with a
  link, `component Ops` (or its stereotype) with a link. Arrows `User ..> Frontend`,
  `Frontend ..> Backend`, `Ops ..> Backend`, `Ops ..> Frontend`, `Support`'s dependency on
  User lifted to `Ops ..> User`. Never a fourth box for "backend services" (trap 2).
- `component/backend`: four components, `"API Server" ..> Database`,
  `"API Server" ..> Queue`, `"API Server" ..> Cache`, `Queue ..> Database`; Frontend
  outside with `Frontend ..> "API Server"`; Ops's parts outside with their arrows into
  Database and API Server. `"API Server"` carries a link down.
- `component/api-server`: three boxes with links down and the arrows among them lifted from
  the classes and handlers: the request handling grouping to the data model grouping
  and to the rendering grouping; the outside Database, Queue, Cache, Frontend, and User
  with arrows into the groupings.
- `class/<data-model-slug>`: in a good run three boxes, Identity, Collaboration, Billing,
  with the lifted arrows among them (Collaboration → Identity from Project →
  Organization and Task → Member; Billing → Identity from Subscription → Organization);
  in a weak run the fourteen classes with every arrow above.
- `class/identity`: `class Account` with its attributes, `Account o-- Session`,
  `Account o-- ApiKey`, `Organization o-- "1..*" Member`, `Member -- Account`; Project
  and Subscription outside with their arrows in.
- `usecase/user-identity`: `actor User`, `usecase "User: Identity"`,
  `User -- "User: Identity"`.
- `sequence/user-identity`: `actor User`, participants Frontend and Backend, one
  message per step that carries an edge; steps between two Backend components draw as
  a self-message on Backend.
- `state/invoice`: `[*] --> open`, `open --> paid`, `open --> overdue`,
  `overdue --> paid`.

Every default carries `default: true` and derived provenance. A no-op rebuild derives
zero goals, makes zero LLM calls, and leaves every view and diagram byte-identical.

## Planted traps

1. Cross-doc identity: API Server, Database, Queue, and Cache are defined in their own
   pages and used in identity.md, collaboration.md, billing.md, monitoring.md,
   deployment.md, and backups.md. Each must be ONE entity with mentions in at least 4
   documents, hence one child each under Backend and one `component/backend` diagram.
2. Lookalike grouping: README.md, backend.md, and api-server.md say "the Backend";
   frontend.md and deployment.md say "the backend services" throughout. Expect reuse of
   `ent:backend` with `backend services` in its aliases, or a `review-entity` or
   `dedupe-candidates` merge, or a `duplicate-entity` diagnostic. A surviving "Backend
   Services" entity draws a fourth box on `component/public` and splits Frontend's
   dependency: a visible failure.
3. The root stays flat: User, Frontend, Backend, Ops are four parentless entities, under
   the soft threshold. No `fan-out` goal derives on `scope:public` and no session mints
   a root grouping (a "Platform" over Frontend and Backend, or an "Infrastructure" over
   Backend and Ops). Making Trellis a root over Frontend and Backend is the same
   failure by another route.
4. Over-hard fan-out that must split: the API Server states twenty-one children, over
   the hard threshold of fifteen. A mandatory `abstract-entity` goal with the `fan-out`
   change derives on `ent:api-server`, and `converged` is blocked while it is open or
   failed. Pass: the session mints groupings named from the headings ("Data model",
   "Request handling", "Rendering") or names the documents already use, each with at
   least two members, derived provenance naming exactly its members, a definition, and
   `parent: ent:api-server`; the API Server lands at three children. Fail: the goal is
   failed as "genuinely flat" (the headings give three areas), or a grouping crosses
   levels by pulling in Queue or Cache, or the session moves classes under a handler.
5. Over-soft fan-out that a good run groups: the data model grouping holds fourteen
   classes, over the soft threshold of nine. An optional `fan-out` goal derives on it.
   Pass (good run): Identity, Collaboration, and Billing minted from the three pages,
   the level at three. Pass (weak run): the goal left open or failed with a reason,
   `optional advised` in the verdict, `converged` unaffected. Fail: a grouping with
   one member, a grouping that names a class (a "Task" grouping over Task and Comment
   while Task stays a member), or the fourteen reparented under Database.
6. Flat level that must not tier: Ops has four peers (Monitoring, Deployment, Backups,
   Support) with no cohesion beyond their parent. No fan-out goal derives, and no
   session may mint a grouping among them (a "Reliability" over Monitoring and Backups,
   a "Delivery" over Deployment). Pass: Ops keeps four children with quote provenance
   through convergence. Fail: any derived entity under Ops, or any of the four
   reparented under Backend.
7. Groupings never cross levels: the Email Templates are a child of the API Server and
   run inside Queue jobs; the Queue stores jobs in the Database. A grouping that puts
   the Email Templates under the Queue, or a grouping that spans a class and a Backend
   component, violates the shared-parent rule and is bounced by `group_entities`. Pass:
   every grouping's members share one parent before and after.
8. Headings are not stated entities: "Data model", "Request handling", and "Rendering"
   are headings in api-server.md, and the lead-in sentences name the API Server as the
   subject ("The API Server owns the following classes", "The API Server has four
   handlers", "The API Server has three renderers"). database.md says "every record the
   API Server owns", never "the data model" as a noun, and README.md's document list
   links file names, so no sentence states an Identity, Collaboration, Billing, or Data
   Model entity. No entity with quote provenance may be anchored on a heading, a
   document title, or those lead-ins. The groupings the
   fan-out session mints carry derived provenance and a ratification proposal toward
   api-server.md; accepting the proposal is the only way they gain quote provenance.
9. Junk bait: environment variables (`TRELLIS_DATABASE_URL`), the path `/srv/trellis/`,
   the commands `trellis migrate` and `trellis rollback`, port `8080`, `/healthz`, the
   cache keys `session:<token>` and `project:<id>`, and the table names in database.md.
   None of these may become entities; they are requirements on Deployment, the API
   Server, the Cache, and the Database with the identifiers kept verbatim. Borderline,
   acceptable either way: the payment provider, the mail provider, the CDN, object
   storage, and the on-call engineer as outside entities.
10. Transitions read from the class pages: Task, Invoice, Account, and Subscription each
    carry their states in prose ("The status of a Task is `open`, `in progress`, or
    `done`") and their transitions in trigger sentences. Expect the four machines above
    with the initial states named. Inventing a `deleted` state for Project from
    "deleting a Project deletes its Tasks" is wrong: the sentence is a cascade, not a
    state.
