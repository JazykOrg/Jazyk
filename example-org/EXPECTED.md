# Org expected outcomes (hand-labeled, not part of the docs glob)

Counts are indicative, not graded: a richer graph is acceptable when every extra
requirement is independently testable and every extra entity has statements directly
about it. The graded part is the traps, the state machine, and the instance.

This fixture is not software. The subject is a company; the deliverable is its employee
handbook (markdown pages under `handbook/`). Stereotypes are the model's judgment, so
the labels below are the expected kind, not a required spelling.

## Core entities (must exist)

- Ridgeline Outfitters, stereotype company, the containment root, no parent.
- Operations, Sales, Finance, People: stereotype department, parent Ridgeline
  Outfitters.
- Store Manager, Warehouse Associate (parent Operations); Sales Associate, Account
  Manager (parent Sales); Accountant, Payroll Specialist (parent Finance); Recruiter,
  People Partner (parent People): stereotype role.
- Team: the type, attributes `lead`, `size`, `members` (from "A team has a lead, a size,
  and a list of members."), no `value` on any of them.
- Boulder Warehouse Crew: an instance of Team, parent Operations (or no parent),
  attributes `lead = Priya Natarajan`, `size = 6`, `members` = the six names.
- Employee, Manager: stereotype actor. Candidate, Hiring Manager: stereotype actor.
- Application (the hiring record), Expense Claim (attributes `total`, `cost center`,
  `receipt` or `receipts`), Employee Handbook (the deliverable, stereotype document or
  handbook).
- Borderline, acceptable either way: Interview Panel, Warehouse, Store, Dana (an
  instance of Sales Associate from the expenses example).
- Must NOT exist: Finance department as a second node (see traps), any form number,
  path, policy number, cost center code, or URL (see traps), the tracker, the expense
  tool, the careers page, the intranet, the shared drive.

## Requirements

At least ~45 across the five documents; no obligation the docs state may be missing.
The handbook section of README.md yields obligations on the handbook (its medium, its
page-per-document layout, its table of contents, its exclusion of form numbers and
paths); it is not navigation.

## Edges (the prose states these)

- Ridgeline Outfitters → each of the four departments: `composition` (README.md list,
  restated in departments.md); the departments' `parent` follows.
- Each department → its two roles: `composition` ("Operations has two roles:").
- Department → Team: `composition`, cardinality `1..*` ("one or more teams").
- Boulder Warehouse Crew → Team: `instantiation`.
- Manager → Employee: `generalization` ("A manager is an employee with direct reports").
- Employee → Department: `association`, cardinality `1`. Employee → Manager:
  `association`, cardinality `1`. Team → Manager: `association` (the lead).
- Hiring: Candidate → Application `association`; Recruiter → Application `dependency`
  (records stage changes, runs the pipeline); Hiring Manager → Interview Panel
  `dependency` when the panel is an entity; Hiring Manager → People `dependency` from
  the hidden rule (trap 4).
- Expenses: Employee → Manager `dependency` (submit); Manager → Finance `dependency`
  (forward); Finance → Employee `dependency` (reimburse, reject); Employee → Expense
  Claim `association`.

## State machine

`sm:application` (subject Application), derived from the "Moving between stages" list:

- states: applied, screened, interviewed, offered, rejected. Initial: applied.
- transitions: applied → screened (recruiter completes the phone screen); applied →
  rejected (recruiter finds no match); screened → interviewed (panel completes the
  loop); screened → rejected (panel declines to run a loop); interviewed → offered
  (hiring manager decides to hire, guard: both references check out); interviewed →
  rejected (hiring manager decides not to hire).
- Exactly one guard, on interviewed → offered. No `nondeterministic-transition`: the
  two exits from interviewed have different triggers.
- `dead-end-state` names offered and rejected. rejected is final. offered is the planted
  gap (trap 7).
- `unhandled-event` fires (every trigger is named from one state only); it is expected
  and not graded.
- A second machine on Expense Claim (approved, returned, rejected) is acceptable but not
  expected; the graded machine is Application.

## Instances and conformance

Boulder Warehouse Crew : Team. Every attribute name on the instance (`lead`, `size`,
`members`) exists on Team. `size = 6` matches the six listed members. Outcome:
conformant, no `nonconformant-instance`. `view:object/team` renders it as
`Boulder Warehouse Crew : Team` with its three values.

## Flow clusters and default views

- hiring.md: one cluster keyed on the actor among each step's entities. When Candidate
  is the only actor label, the cluster is `candidate-hiring`, title "Candidate: Hiring",
  and every stage change is a member. A split into a Recruiter cluster and a Hiring
  Manager cluster is acceptable when each has two or more members. Each cluster with two
  or more members yields `view:usecase/<cluster>` and `view:sequence/<cluster>` sharing
  a title.
- expenses.md: between one and three clusters (Employee, Manager, Finance, by which
  actor each step lists first). "Employee: Expenses" is expected with the submit and
  resubmit steps at least. The two `failure-mode` steps (missing receipt, non-business
  line) are members of some cluster, so no `unrepresented-failure-mode`.
- Structural defaults are level views, one per node with two or more children, the
  scope root included (docs/compiler/model/view.md#level-views); members are the direct
  children plus every outside entity with a lifted edge into the level, no `query`.
  - `view:component/public` (one scope expected), the scope root's level view under
    the per-scope id: the parentless entities, Ridgeline Outfitters beside Employee,
    Manager, Candidate, Hiring Manager, Team, Application, Expense Claim, and the
    Employee Handbook, never "every entity" (the departments sit in Ridgeline
    Outfitters' level). Kind `component` because the parentless actors (Candidate,
    Hiring Manager, Employee, Manager) carry `actor`; `view:class/public` with the
    same members when no parentless entity carries a structural stereotype. Both are
    mechanism-faithful.
  - `view:class/ridgeline-outfitters`: the four departments (the node's children) plus
    the outside entities whose edges lift into the level: Employee (associated to a
    Department), Manager (forwards to Finance), Team when it sits outside (a Department
    composes Team). Kind `class` unless a department carries a structural stereotype.
  - `view:class/<department>` for each department with two or more children (its two
    roles, and Team or Boulder Warehouse Crew when parented there):
    `view:class/operations`, `view:class/sales`, `view:class/finance`,
    `view:class/people`. Members: the roles plus lifted interactors (Candidate and
    Application into People through the Recruiter; Employee into Finance through the
    reimbursement).
  - Deeper flow views cluster the behavior requirements that lift into a level under
    `view:usecase/<node-slug>-<cluster-slug>` and `view:sequence/...`
    (`view:usecase/ridgeline-outfitters-employee-expenses` when the expenses steps lift
    into the departments' level). The root form keeps the unprefixed ids above.
    Mechanism-faithful surplus, not graded.
- `view:state/application`, `view:object/team`.
- No timing view derives (timing views are curated). The time bounds in hiring.md
  ("within 5 business days of the application") are what a curated timing view over
  Application would read.

## Planted traps

1. Cross-doc identity: `Employee` is defined in departments.md and used in hiring.md,
   expenses.md, policies.md. ONE entity with mentions in at least 3 documents. The same
   for `Manager` (departments.md, hiring.md, expenses.md, policies.md).
2. Lookalike pair: departments.md and expenses.md say "Finance"; policies.md says "the
   Finance department" throughout. Expect reuse of `ent:finance` with the alias, or a
   merge or `duplicate-entity` diagnostic. Two surviving nodes is the failure.
3. Contradiction: expenses.md says Finance approves claims above 500 dollars;
   policies.md says above 250 dollars, for the same approval. Expect exactly one
   `contradiction` diagnostic naming both requirements, on Expense Claim or Finance.
4. Non-normative trap: hiring.md "Background" is history and hides one rule ("No offer
   goes out before People has approved the salary band for the role."). Marking the
   section non-normative must trigger `suspicious-non-normative`; extracting the rule
   and marking the section covered is also correct.
5. Junk bait: form numbers (HR-04, EX-12, HR-21), paths (`People/Hiring/Requisitions/`,
   `S:\Finance\Claims\<year>\<employee>`), policy numbers (POL-101 to POL-107), cost
   center codes (`CC-4410`), the careers URL. None of these may become entities. The
   README's rule that the handbook never mentions them is a requirement on the handbook.
6. Genuinely non-normative: policies.md "Glossary" defines outside-world terms only and
   must be marked non-normative without a `suspicious-non-normative` finding.
7. Dead end: hiring.md lists hired as a stage, but no sentence moves an application into
   it (offered → hired on the candidate accepting is the deliberately missing
   transition). `sm:application` has no hired state, offered has no exit, and
   `dead-end-state` names it. Inventing the transition from the stage list is wrong;
   the gap is for the document owner to close.
8. Quality without a measure: policies.md "Sales answers every customer email promptly."
   is a `quality` facet with no `measure` and draws `quality-unmeasured`. Every other
   quality rule in policies.md carries its bound as `measure` (10 days, 2 days per week,
   8 direct reports, 30 days, once per quarter, 2 business days).

## Diagrams

- The root level (`view:component/public`): the parentless entities, actors as
  `actor`, the rest as `component`. Ridgeline Outfitters with its lifted arrows (the
  departments' composition of Team lifts to the company:
  `"Ridgeline Outfitters" *-- "1..*" Team` when Team is parentless,
  `"Ridgeline Outfitters" -- Employee` from the Employee-to-Department association);
  Manager generalizes Employee; Employee associated to Manager. When the root derives as
  `class/public`, Team draws as a class with `lead`, `size`, `members`. Instances
  (Boulder Warehouse Crew, Dana) are absent from every level view. Ridgeline
  Outfitters' element carries the drill-down link
  `[[../class/ridgeline-outfitters.svg]]`.
- The company's level (`view:class/ridgeline-outfitters`): the four departments as
  classes, each composed of its roles and of Team (`1..*`) as lifted arrows to the
  department, plus the lifted interactors (Employee, Manager). A department with two or
  more children carries its link down to `view:class/<department>`, where the roles
  draw as classes with the department's own arrows.
- Object (`view:object/team`): one object `Boulder Warehouse Crew : Team` with
  `lead = Priya Natarajan`, `size = 6`, `members = ...`.
- No separate component view: the per-root component view of earlier designs folded
  into the level rule, and no entity here carries a structural stereotype unless the
  compile records the actors as such (then the levels holding them derive as
  `component` views with the same members).
- State (`view:state/application`): `[*] --> applied`, six arrows, the offered arrow
  labeled with its trigger and `[both references check out]`, no arrow leaving offered,
  no hired node.
- Use case and sequence per cluster: hiring shows Candidate (and the recruiter, panel,
  hiring manager as participants); expenses shows messages employee → manager,
  manager → Finance, Finance → employee.
