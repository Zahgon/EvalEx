//! `java.util.regex` on UTF-16 strings: `Pattern.compile` and `Matcher.matches`.
//!
//! The engine reproduces the observable behaviour of JDK 21, including its error messages and the
//! way its backtracking treats quantified groups.

mod bracket;
mod charclass;
mod escape;
mod matcher;
mod node;
mod parser;
mod property;
mod study;
mod unicode;
mod unicode_tables;

use super::{js, JResult, JString, Thrown};
use node::Program;

/// A syntax error in a pattern: the state of a `java.util.regex.PatternSyntaxException`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternSyntaxError {
    /// `getDescription()`.
    pub description: JString,
    /// `getIndex()`, `-1` when no position is known.
    pub index: i32,
    /// `getPattern()`.
    pub pattern: JString,
}

impl PatternSyntaxError {
    /// `getMessage()`: description, position, the pattern and a caret line under it.
    pub fn message(&self) -> JString {
        let mut message = self.description.clone();
        if self.index >= 0 {
            message.extend(js(&format!(" near index {}", self.index)));
        }
        message.push(u16::from(b'\n'));
        message.extend_from_slice(&self.pattern);
        if let Ok(index) = usize::try_from(self.index) {
            if index < self.pattern.len() {
                message.push(u16::from(b'\n'));
                let tab = u16::from(b'\t');
                let blank = |&unit: &u16| if unit == tab { tab } else { u16::from(b' ') };
                message.extend(self.pattern.iter().take(index).map(blank));
                message.push(u16::from(b'^'));
            }
        }
        message
    }
}

/// A compiled regular expression.
#[derive(Clone, Debug)]
pub struct Pattern {
    program: Program,
}

impl Pattern {
    /// `Pattern.compile(pattern)`.
    pub fn compile(pattern: &[u16]) -> Result<Pattern, PatternSyntaxError> {
        let program = parser::Parser::compile(pattern)?;
        Ok(Pattern { program })
    }

    /// `matcher(input).matches()`: whether the whole of `input` matches.
    pub fn matches(&self, input: &[u16]) -> JResult<bool> {
        matcher::matches(&self.program, input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compiled(pattern: &str) -> Pattern {
        Pattern::compile(&js(pattern)).expect("valid pattern")
    }

    fn is_match(pattern: &str, input: &str) -> bool {
        compiled(pattern).matches(&js(input)).expect("no exception")
    }

    fn message(pattern: &str) -> String {
        let error = Pattern::compile(&js(pattern)).expect_err("invalid pattern");
        String::from_utf16_lossy(&error.message())
    }

    #[test]
    fn matches_whole_input_only() {
        assert!(is_match("a+b", "aab"));
        assert!(!is_match("a+", "aab"));
        assert!(is_match("", ""));
    }

    #[test]
    fn quantifiers_backtrack_or_not() {
        assert!(is_match("a*a", "aaa"));
        assert!(!is_match("a*+a", "aaa"));
        assert!(is_match("(a|ab)*?b", "abab"));
        assert!(!is_match("(?>a|ab)c", "abc"));
        assert!(is_match("(a*a)+a", "aaa"));
    }

    #[test]
    fn counted_closures_after_nothing_are_legal() {
        assert!(is_match("{2}", ""));
        assert!(is_match("a{2}{3}", "aa"));
    }

    #[test]
    fn failed_atomic_groups_keep_their_captures() {
        assert!(is_match("(?:(?>(a))x|a\\1)", "aa"));
    }

    #[test]
    fn line_ending_is_atomic_only_when_quantified() {
        assert!(is_match("\\R\\n", "\r\n"));
        assert!(!is_match("\\R+\\n", "\r\n"));
    }

    #[test]
    fn back_references_take_digits_while_groups_exist() {
        assert!(is_match("(a)\\11", "aa1"));
        assert!(!is_match("\\1", ""));
        assert!(is_match("(?<n>a)\\k<n>", "aa"));
    }

    #[test]
    fn class_operators() {
        assert!(is_match("[a-c&&[^b]]", "c"));
        assert!(!is_match("[a-c&&[^b]]", "b"));
        assert!(!is_match("[ab\\d&&]", "a"));
        assert!(is_match("[\\[^a]", "^"));
    }

    #[test]
    fn surrogates_are_never_split() {
        assert!(!is_match("\\uD83D.", "\u{1F600}"));
        assert!(is_match("\\x{1F600}", "\u{1F600}"));
    }

    #[test]
    fn case_folding_depends_on_flags() {
        assert!(!is_match("(?i)\u{E9}", "\u{C9}"));
        assert!(is_match("(?iu)\u{E9}", "\u{C9}"));
        assert!(is_match("(?iu)k", "\u{212A}"));
    }

    #[test]
    fn look_behind_lengths_wrap_like_java_ints() {
        assert!(is_match("aa(?<=^a*)b", "aab"));
        assert!(!is_match("(?<=a*b*)", ""));
        assert_eq!(
            message("(?<=(ab)*)"),
            "Look-behind group does not have an obvious maximum length near index 8\n\
             (?<=(ab)*)\n        ^"
        );
    }

    #[test]
    fn grapheme_boundary_measures_from_the_last_sub_match() {
        assert!(is_match("a\\b{g}b", "ab"));
        assert!(!is_match("y{2,}+\\b{g}a", "yya"));
        let thrown = compiled("(?=a+).\\b{g}.").matches(&js("aa"));
        assert_eq!(
            thrown,
            Err(Thrown::new("java.lang.StringIndexOutOfBoundsException", "Index 2 out of bounds for length 2"))
        );
    }

    #[test]
    fn error_messages() {
        assert_eq!(message("*"), "Dangling meta character '*' near index 0\n*\n^");
        assert_eq!(message("("), "Unclosed group near index 1\n(");
        assert_eq!(message(")"), "Unmatched closing ')'\n)");
        assert_eq!(message("a\\"), "Unescaped trailing backslash near index 2\na\\");
        assert_eq!(message("[z-a]"), "Illegal character range near index 3\n[z-a]\n   ^");
        assert_eq!(message("\\p{Foo}"), "Unknown character property name {Foo} near index 6\n\\p{Foo}\n      ^");
        assert_eq!(message("\ta{,}"), "Illegal repetition near index 3\n\ta{,}\n\t  ^");
    }

    #[test]
    fn long_input_does_not_exhaust_the_stack() {
        let input = js(&"ab".repeat(200_000));
        assert_eq!(compiled("(a|b)*").matches(&input), Ok(true));
        assert_eq!(compiled("(?:a|b(?=a|$))*?").matches(&input), Ok(true));
    }

    #[test]
    fn deep_nesting_is_rejected_not_fatal() {
        let deep = "(".repeat(100_000);
        assert!(Pattern::compile(&js(&deep)).is_err());
        let fine = format!("{}a{}", "(".repeat(200), ")".repeat(200));
        assert!(is_match(&fine, "a"));
    }
}
