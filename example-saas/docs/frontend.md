# Frontend

The Frontend is a single-page web application written in TypeScript. It is the only
part of the product a [User](user.md) sees. The Frontend calls the backend services over
HTTPS and holds no data of its own beyond the current [Session](identity.md#session)
token.

## Behavior

- The Frontend keeps the Session token in memory and sends it with every request to the
  backend services.
- The Frontend signs the User out when a request is refused with `401`.
- The Frontend shows every Project of the current Organization on the home page,
  ordered by last activity.
- The Frontend shows a Task's Comments in the order they were written.
- The Frontend uploads an Attachment directly to the backend services in one request of
  at most 25 MB.
- The Frontend renders every page within 200 milliseconds after the response arrives.

## Pages

The Frontend has the following pages:

- the signup and login pages
- the home page listing the Projects of the current Organization
- a Project page listing its Tasks
- a Task page with the Task's Comments and Attachments
- the Organization settings page, where an owner invites Members and changes the Plan
- the billing page listing the Organization's Invoices
- the help menu, which opens a Support ticket form

## Delivery

The Frontend is built into static files and served from a CDN. A new build of the
Frontend is published by [Deployment](deployment.md) together with the backend services.
