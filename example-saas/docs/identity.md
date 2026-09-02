# Identity

This document describes the classes that say who a [User](user.md) is and where the
User belongs: Account, Organization, Member, Session, and ApiKey. The
[API Server](api-server.md) persists all five in the [Database](database.md); the
[Auth Handler](api-server.md#request-handling) and the
[Organization Handler](api-server.md#request-handling) write them.

## Account

An Account is the login of one User. An Account has an email address, a display name,
a password hash, and a status. The email address of an Account is unique across all
Accounts. The status of an Account is `unverified`, `verified`, or `suspended`. An
Account is created in the status `unverified`. When the User opens the verification
link, the Account becomes `verified`. The Auth Handler marks an Account `suspended`
after 10 failed logins in an hour. A password reset returns a `suspended` Account to
`verified`. An Account has zero or more Sessions and zero or more ApiKeys.

## Organization

An Organization is the workspace that owns Projects and holds a Subscription. An
Organization has a name and a slug. The slug of an Organization is unique across all
Organizations and appears in every URL of the Organization. An Organization has one or
more Members and at least one of them is an owner. An Organization holds exactly one
[Subscription](billing.md#subscription). Deleting an Organization deletes its Projects,
Members, and Invoices after a 30 day grace period.

## Member

A Member ties one Account to one Organization with a role. The role of a Member is
`owner` or `contributor`. An Account has at most one Member record per Organization. The
Organization Handler refuses to remove the last owner of an Organization. When a Member
is removed, the Tasks assigned to that Member become unassigned.

## Session

A Session is a signed-in Account on one browser. A Session has a token, an Account, and
an expiry. A Session token is a random 256 bit value the Auth Handler issues at login.
A Session expires 30 days after login or when the User logs out. The API Server looks
up a Session in the [Cache](cache.md) first and in the Database when the Cache has no
entry.

## ApiKey

An ApiKey is a long-lived token that lets a script act as an Account. An ApiKey has a
name, a hashed secret, an Account, and an optional expiry. The API Server shows the
secret of an ApiKey once, at creation, and stores only its hash. A request carrying an
ApiKey has the permissions of the ApiKey's Account. An Account has at most 10 ApiKeys.
The User revokes an ApiKey from the Organization settings page, and a revoked ApiKey
is refused on its next use.

## Signup

Signup creates an Account for a new User:

1. The User submits the signup form in the [Frontend](frontend.md) with an email
   address and a password.
2. The Frontend posts the form to the API Server.
3. The Auth Handler creates the Account in the Database in the status `unverified`.
4. The Auth Handler enqueues a verification email job on the [Queue](queue.md).
5. The [Email Templates](api-server.md#rendering) render the verification email with a
   link that expires after 7 days.
6. When the User opens the verification link, the Auth Handler marks the Account
   `verified`, creates an Organization named after the User, and adds the User as its
   owner.
7. The Auth Handler issues a Session, and the Frontend shows the home page.

When the email address is already taken, the Auth Handler answers `422` and the
Frontend shows the login page instead.

## Inviting a member

An owner adds another person to an Organization:

1. The User enters an email address in the invite form of the Organization settings
   page in the Frontend.
2. The Frontend posts the invitation to the API Server.
3. The Organization Handler checks that the User is an owner of the Organization.
4. The Organization Handler enqueues an invitation email job on the Queue.
5. The Email Templates render the invitation email with a link that expires after 7
   days.
6. When the invited person opens the link, the Auth Handler creates or finds the
   Account for that email address.
7. The Organization Handler creates a Member record with the role `contributor` in
   the Database and writes an AuditEntry.

When the invited email address already has a Member record in the Organization, the
Organization Handler answers `422` and sends no email.
