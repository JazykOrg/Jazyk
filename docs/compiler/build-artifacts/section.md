# Section (Build artifact)

Each documentation section creates a Section build artifact. This artifact documents relevant
information about the section that can be used for re-compilation and downstream processing
such as code-generation.

## Location

Each section has a location reference that can be used to locate the section in the documentation.

The location reference is a URI that can be used to locate the section in the documentation, the
path to the section in the file itself is specific to the documentation format.

As an example, for documentation such as:

`<path>/landing-page/registration.md`:
```markdown
# Registration
...
## Required fields
...
1. **Email**: Email from the company domain
2. ...
```

A section for the email required field would be located as such:

``./target/.../landing-page/registration/registration-required-fields-email.json``:
```json
{
  "location": "file://<path>/landing-page/registration.md#/registration/required-fields/0"
}
```

## Content hash

The content hash (`contentHash`) is a `MurmurHash3` hash of the section content that can be used to determine if the section has
changed since the last build.

## Relationship

This section may have a relationship to another section.

See [types of relationships](./relationships.md#relationships) for more information.

## JSON Schema

The Section build artifact follows [this schema](./section.schema.yaml).
