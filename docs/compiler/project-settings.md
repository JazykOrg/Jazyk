# Project Settings

## Location

The project settings are stored in a natlan.yml file.

## Docs

The settings includes how to manage documentation files.

### Glob

The pattern to match or exclude documentation files can be defined as:

```yaml
docs:
  glob:
    - "docs/**/*.md"
    - "!docs/api/**/*.md"
```

### Handlers

In addition to built-in handlers, you can define your own handlers for
custom documentation files here.

TODO path to docs
TODO matcher for docs (e.g. ignore certain files or include only certain files)
TODO plugin system on how to read certain types of files e.g.: markdown, docx, drawio, UML/XMI

### Linting

TODO:
- Define linting rules such as:
  - Whether to check grammatical errors
  - Any open-ended rules such as terminology, flow, formats etc...

## Schema

The JSON schema for the project settings is [located here](./project-settings.schema.yaml).
