//! Parser diagnostics for rejected native tool arguments. Never repairs a proposal.

pub(crate) fn json_error_feedback(
    name: &str,
    arguments: &str,
    finish_reason: Option<&str>,
) -> Option<String> {
    let error = serde_json::from_str::<serde_json::Value>(arguments).err()?;
    let line_start: usize = arguments
        .split_inclusive('\n')
        .take(error.line().saturating_sub(1))
        .map(str::len)
        .sum();
    let mut byte_offset = line_start
        .saturating_add(error.column().saturating_sub(1))
        .min(arguments.len());
    while !arguments.is_char_boundary(byte_offset) {
        byte_offset -= 1;
    }
    let character_offset = arguments[..byte_offset].chars().count();
    let start = character_offset.saturating_sub(64);
    let excerpt: String = arguments.chars().skip(start).take(128).collect();
    let characters = arguments.chars().count();
    let omitted = characters.saturating_sub(excerpt.chars().count());
    let reason = finish_reason.unwrap_or("not reported");
    let limit = if finish_reason == Some("length") {
        "The provider reported an output token limit."
    } else {
        "The shortened excerpt does not imply provider output truncation."
    };
    Some(format!(
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/inline/harness-tool_arguments/tool-name-arguments-are-invalid-json-error-9a94f.md"
        )),
        arguments.len(),
        name = name,
        error = error,
        characters = characters,
        omitted = omitted,
        excerpt = excerpt,
        reason = reason,
        limit = limit
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_regex_arguments_report_invalid_escape_instead_of_truncation() {
        let arguments = format!(
            r#"{{"path":"tests/pattern.py","content":"{}r'\w+\.py'"}}"#,
            "前".repeat(1_000)
        );
        let error = json_error_feedback("write_text_file", &arguments, Some("tool_calls")).unwrap();
        assert!(error.contains("invalid escape at line 1 column"), "{error}");
        assert!(error.contains("\\\\w"), "{error}");
        assert!(error.contains("Provider finish_reason: tool_calls"));
        assert!(error.contains("does not imply provider output truncation"));
        assert!(error.contains("characters (") && error.contains("bytes)"));
        assert!(error.len() < 1_500, "error should remain bounded");
    }

    #[test]
    fn actual_output_limit_and_missing_finish_reason_stay_distinct() {
        let arguments = r#"{"content":"unfinished"#;
        let limited = json_error_feedback("write_text_file", arguments, Some("length")).unwrap();
        assert!(limited.contains("EOF while parsing a string"));
        assert!(limited.contains("The provider reported an output token limit"));
        let unknown = json_error_feedback("write_text_file", arguments, None).unwrap();
        assert!(unknown.contains("finish_reason: not reported"));
        assert!(!unknown.contains("provider reported an output token limit"));
    }

    #[test]
    fn valid_multiline_source_has_no_json_error_feedback() {
        let source = "pattern = r'\\w+\\.py'\nprint(\"ok\")\n";
        let arguments = serde_json::json!({"content":source}).to_string();
        assert!(json_error_feedback("write_text_file", &arguments, Some("tool_calls")).is_none());
        assert!(json_error_feedback("tool", "[]", Some("tool_calls")).is_none());
    }
}
