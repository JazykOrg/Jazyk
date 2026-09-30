# Application

A candidate's application against a requisition in the hiring pipeline.

## Stages

An application is in exactly one stage at a time. The stages are:

An application is in exactly one stage at a time; the stages below are mutually
exclusive, so an application shown in `applied` is not simultaneously shown in
`screened`, `interviewed`, `offered`, `hired`, or `rejected`.

- applied: the candidate submitted the application through the careers page at
  `ridgeline.example/careers`.
- screened: the recruiter has finished the phone screen.
- interviewed: the interview panel has finished the loop.
- offered: an offer letter is with the candidate.
- hired: the candidate is on the payroll.
- rejected: the application is closed and the candidate has been told.

## Moving between stages

- An applied application becomes screened when the recruiter completes the phone
  screen with the candidate.
- An applied application becomes rejected when the recruiter finds no match at the
  phone screen.
- A screened application becomes interviewed when the interview panel completes the
  loop with the candidate.
- A screened application becomes rejected when the interview panel declines to run a
  loop.
- An interviewed application becomes offered when the hiring manager decides to hire,
  provided both of the candidate's references check out.
- An interviewed application becomes rejected when the hiring manager decides not to
  hire.

The recruiter records every stage change with its date and the person who made it.
The recruiter tells the candidate about every stage change within 2 business days.

## Timing

The recruiter completes the phone screen within 5 business days of the application.
