# Collaboration

This document describes the classes a [User](user.md) works with day to day: Project,
Task, Comment, Attachment, Notification, and AuditEntry. The
[API Server](api-server.md) persists all six in the [Database](database.md); the
[Project Handler](api-server.md#request-handling) writes the first four, and every
handler writes Notifications and AuditEntries.

## Project

A Project is a named container of Tasks in an [Organization](identity.md#organization).
A Project has a name, a description, and an Organization. The name of a Project is
unique within its Organization. A Project belongs to exactly one Organization. Only an
owner deletes a Project, and deleting a Project deletes its Tasks. The API Server caches
a Project with its Tasks in the [Cache](cache.md) for 10 minutes.

## Task

A Task is a unit of work in a Project. A Task has a title, a status, an optional
assignee, and an optional due date. The status of a Task is `open`, `in progress`, or
`done`. A Task is created in the status `open`. When the assignee starts the Task, it
becomes `in progress`. When the assignee completes the Task, it becomes `done`. A
[Member](identity.md#member) reopens a `done` Task, which puts it back in `open`. The
assignee of a Task is a Member of the Project's Organization. A Task belongs to exactly
one Project. A Project has at most 5000 Tasks.

## Comment

A Comment is a message written on a Task. A Comment has a body, an author, and a Task.
The author of a Comment is a Member. A Comment body is at most 10000 characters. The
author edits a Comment for 10 minutes after writing it, and the API Server refuses an
edit after that. A Comment that mentions a Member by `@name` creates a Notification for
that Member's Account.

## Attachment

An Attachment is a file uploaded to a Task. An Attachment has a file name, a size, a
content type, and a Task. An Attachment is at most 25 MB. The API Server stores the
bytes of an Attachment in object storage and the metadata in the Database. Deleting a
Task deletes its Attachments.

## Notification

A Notification is a message to an [Account](identity.md#account) about an event in an
Organization the Account belongs to. A Notification has a recipient Account, an event
text, a link, and a read flag. The link of a Notification opens the Task, Comment, or
Invoice the event concerns. The API Server creates a Notification when a Task is
assigned to a Member, when a Comment mentions a Member, and when an Invoice becomes
overdue. The [Frontend](frontend.md) shows unread Notifications in the header. The API
Server emails unread Notifications to the recipient as a digest at most once per hour
through the [Queue](queue.md).

## AuditEntry

An AuditEntry is a record of who changed what in an Organization. An AuditEntry has an
actor, an action, a target record, an Organization, and a timestamp. The actor of an
AuditEntry is a Member or an ApiKey. The target record of an AuditEntry is a Project, a
Task, a Comment, an Attachment, a Member, or an Invoice. Every handler writes an
AuditEntry for every write
it performs. An AuditEntry is never edited or deleted before its retention ends. An
owner reads the AuditEntries of the Organization from the Organization settings page.

## Creating a project

A User starts work in a new Project:

1. The User opens the new project form in the Frontend and enters a name.
2. The Frontend posts the form to the API Server.
3. The Project Handler creates the Project in the User's current Organization in the
   Database.
4. The Project Handler writes an AuditEntry for the creation.
5. The User adds a Task by entering a title on the Project page in the Frontend.
6. The Project Handler stores the Task in the status `open` in the Database and deletes
   the Project's entry from the Cache.
7. The User assigns the Task to a Member, and the API Server creates a Notification for
   that Member.
8. The Frontend shows the Project with its Tasks, served from the Cache when the entry
   is present.

When the name is already used in the Organization, the Project Handler answers `422`
and the Frontend keeps the form open with the error.
