# User

A User is a person who uses the product through the [Frontend](frontend.md). Every User
holds exactly one [Account](identity.md#account). A User belongs to one or more
[Organizations](identity.md#organization) through a [Member](identity.md#member) record
per Organization.

## What a User can do

- A User signs up with an email address and a password. See
  [Signup](identity.md#signup).
- A User creates [Projects](collaboration.md#project) in any Organization where the
  User is a Member. See [Creating a project](collaboration.md#creating-a-project).
- A User adds [Tasks](collaboration.md#task) to a Project and assigns them to Members.
- A User comments on a Task and attaches files to it.
- A User invites another person into an Organization by email. See
  [Inviting a member](identity.md#inviting-a-member).
- A User pays the [Invoices](billing.md#invoice) of an Organization where the User is
  an owner. See [Paying an invoice](billing.md#paying-an-invoice).
- A User opens a [Support](support.md) ticket from the help menu of the Frontend.

## Roles

A User is either an owner or a contributor in each Organization, recorded on the Member
record. An owner can invite Members, change the Plan, and pay Invoices. A contributor can
create Projects and Tasks and comment on them. Only an owner can delete a Project.

## Limits

A User can be a Member of at most 20 Organizations. A User receives at most one
[Notification](collaboration.md#notification) email per hour; further Notifications in
that hour are batched into the next email.
