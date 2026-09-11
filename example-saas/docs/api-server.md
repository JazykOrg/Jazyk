# API Server

The API Server is the service in the [Backend](backend.md) that handles every request
from the [Frontend](frontend.md). It is a Go program exposing a JSON API over HTTPS on
port `8080`. The API Server checks the [Session](identity.md#session) token or the
[ApiKey](identity.md#apikey) on every request except signup, login, and the health
check. The API Server answers the health check at `/healthz` with `200` while it can
reach the [Database](database.md).

## Data model

The API Server owns the following classes and persists each of them in the Database:

- [Account](identity.md#account): a login, the credentials of one User.
- [Organization](identity.md#organization): a workspace that owns Projects and holds a
  Subscription.
- [Member](identity.md#member): the record tying one Account to one Organization with a
  role.
- [Session](identity.md#session): a signed-in Account on one browser.
- [ApiKey](identity.md#apikey): a long-lived token for scripts acting as an Account.
- [Project](collaboration.md#project): a named container of Tasks in an Organization.
- [Task](collaboration.md#task): a unit of work in a Project.
- [Comment](collaboration.md#comment): a message written on a Task.
- [Attachment](collaboration.md#attachment): a file uploaded to a Task.
- [Notification](collaboration.md#notification): a message to an Account about an
  event in an Organization.
- [AuditEntry](collaboration.md#auditentry): a record of who changed what in an
  Organization.
- [Plan](billing.md#plan): a priced tier of the product.
- [Subscription](billing.md#subscription): an Organization's standing order for a Plan.
- [Invoice](billing.md#invoice): the amount due for one billing period of a
  Subscription.

Every class has an `id` that is a UUID assigned by the API Server at creation and a
`created_at` timestamp. The API Server never reuses an `id`.

## Request handling

The API Server has four handlers and routes every request by its path prefix to one of
them:

- The Auth Handler serves `/auth/*`: signup, login, logout, email verification, and
  password reset.
- The Organization Handler serves `/orgs/*`: creating an Organization, inviting and
  removing Members, and changing a Member's role.
- The Project Handler serves `/projects/*`: Projects, Tasks, Comments, and
  Attachments.
- The Billing Handler serves `/billing/*`: Plans, Subscriptions, Invoices, and the
  payment provider's webhooks.

Every handler checks that the caller is a Member of the Organization the request names
before reading or writing anything. Every handler writes an AuditEntry for each write
it performs. A handler answers `404` for a record the caller cannot see, never `403`.
A handler answers `422` with a list of field errors when the request body fails
validation.

## Rendering

The API Server has three renderers for its responses and outgoing messages:

- The JSON Serializer turns every record into the JSON shape the Frontend reads. It
  omits fields the caller's role cannot see, and it renders every timestamp in UTC
  with the `Z` suffix.
- The Email Templates render every email the product sends: verification, invitation,
  Notification digest, and Invoice emails. Each email has a plain text part and an
  HTML part, and every link in an email carries a signed token that expires after 7
  days.
- The CSV Exporter renders a Project's Tasks as a CSV file with one row per Task and
  the columns `id`, `title`, `status`, `assignee`, `created_at`. An export runs as a
  job on the [Queue](queue.md) and the finished file is stored as an Attachment on the
  Project.

## Errors

The API Server logs every request with its path, status, and duration. The API Server
answers every request within 500 milliseconds at the 95th percentile. When a handler
panics, the API Server answers `500` and reports the panic to [Monitoring](monitoring.md).
