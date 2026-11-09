# reStructuredText grammar

`rst.pest` is copied from `rst_parser` 0.4.2,
https://github.com/flying-sheep/rust-rst/tree/a59c330d2109e823b0c9ac791e5216951987711d/parser.
The upstream crate is licensed MIT OR Apache-2.0.
Jazyk uses the grammar through Pest to retain exact source spans.
No upstream Rust parser implementation is copied.
The literal-block rule also accepts a paragraph ending in `::`. This keeps
ordinary reStructuredText literal introductions in the source span and excludes
their indented body from heading and reference extraction.
