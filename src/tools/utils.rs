//! Shared tool helpers: path shortening, output truncation, etc.

use std::path::Path;

/// Shorten a path for display, preserving the last N components.
pub fn shorten_path(path: &Path, max_len: usize) -> String {
    let display = path.display().to_string();
    if display.len() <= max_len {
        return display;
    }
    // Keep the last 3 components
    let components: Vec<&str> = display.split('/').collect();
    if components.len() <= 3 {
        return display;
    }
    let tail: Vec<&str> = components.iter().rev().take(3).rev().copied().collect();
    format!(".../{}", tail.join("/"))
}

/// Truncate a long string for the LLM, adding a note about length.
pub fn truncate_output(output: &str, max_lines: usize, max_chars: usize) -> String {
    let lines: Vec<&str> = output.lines().collect();

    if lines.len() <= max_lines && output.len() <= max_chars {
        return output.to_string();
    }

    let mut result = String::new();
    let mut char_count = 0;
    let mut truncated = false;

    for line in lines.iter().take(max_lines) {
        if char_count + line.len() > max_chars {
            truncated = true;
            break;
        }
        result.push_str(line);
        result.push('\n');
        char_count += line.len() + 1;
    }

    if truncated || lines.len() > max_lines {
        result.push_str(&format!(
            "\n[Output truncated: {} lines, {} chars total. Full output in temp file.]",
            lines.len(),
            output.len()
        ));
    }

    result
}

/// Format a file with line numbers (standard read output).
pub fn format_with_line_numbers(content: &str, start_line: usize) -> String {
    content
        .lines()
        .enumerate()
        .map(|(i, line)| format!("{:>6}: {}", start_line + i, line))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shorten_path_short_enough() {
        let path = Path::new("/home/user/project");
        let result = shorten_path(path, 50);
        assert_eq!(result, "/home/user/project");
    }

    #[test]
    fn test_shorten_path_too_long() {
        let path = Path::new("/very/long/path/that/exceeds/max_len/project/src/main.rs");
        let result = shorten_path(path, 20);
        assert!(result.starts_with(".../"));
        assert!(result.contains("main.rs"));
    }

    #[test]
    fn test_truncate_output_within_limits() {
        let text = "line1\nline2\nline3";
        assert_eq!(truncate_output(text, 10, 500), text);
    }

    #[test]
    fn test_truncate_output_exceeds_lines() {
        let text = "a\nb\nc\nd\ne\nf";
        let result = truncate_output(text, 3, 500);
        assert!(result.contains("truncated"));
        assert!(result.contains("6 lines"));
    }

    #[test]
    fn test_truncate_output_exceeds_chars() {
        let text = "a\nb\nc";
        let result = truncate_output(text, 10, 2);
        assert!(result.contains("truncated"));
    }

    #[test]
    fn test_format_with_line_numbers() {
        let content = "hello\nworld";
        let result = format_with_line_numbers(content, 1);
        assert_eq!(result, "     1: hello\n     2: world");
    }

    #[test]
    fn test_format_with_line_numbers_offset() {
        let content = "fn main() {\n    println!();\n}";
        let result = format_with_line_numbers(content, 42);
        assert!(result.starts_with("    42: fn main() {"));
        assert!(result.contains("    44: }"));
    }
}
