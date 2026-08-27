use std::{fs, io::ErrorKind, path::Path};

use anyhow::{Context, Result, bail};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitignoreDocument {
    pub lines: Vec<String>,
    line_ending: String,
    trailing_line_ending: bool,
}

impl GitignoreDocument {
    pub fn from_text(text: &str) -> Self {
        let line_ending = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let trailing_line_ending = text.ends_with(line_ending);
        let body = if trailing_line_ending {
            &text[..text.len() - line_ending.len()]
        } else {
            text
        };
        let lines = if body.is_empty() {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![String::new()]
            }
        } else {
            body.split(line_ending).map(ToOwned::to_owned).collect()
        };
        Self {
            lines,
            line_ending: line_ending.to_owned(),
            trailing_line_ending,
        }
    }

    pub fn to_text(&self) -> String {
        if self.lines.is_empty() {
            return String::new();
        }
        let mut text = self.lines.join(&self.line_ending);
        if self.trailing_line_ending {
            text.push_str(&self.line_ending);
        }
        text
    }

    pub fn add_rule(&mut self) {
        self.lines.push(String::new());
        self.trailing_line_ending = true;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitignoreExplanation {
    pub zh: String,
    pub en: String,
}

pub fn load_repository_gitignore(root: &Path) -> Result<(GitignoreDocument, String)> {
    let path = root.join(".gitignore");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    Ok((GitignoreDocument::from_text(&text), text))
}

pub fn save_repository_gitignore(
    root: &Path,
    expected_original: &str,
    document: &GitignoreDocument,
) -> Result<()> {
    let path = root.join(".gitignore");
    let current = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    if current != expected_original {
        bail!(".gitignore changed on disk while it was being edited; reload it before saving");
    }
    fs::write(&path, document.to_text()).with_context(|| format!("write {}", path.display()))
}

pub fn explain_gitignore_line(line: &str) -> GitignoreExplanation {
    if line.trim().is_empty() {
        return explanation(
            "空行，不产生忽略规则。",
            "Blank line; it does not create an ignore rule.",
        );
    }
    if line.starts_with('#') {
        return explanation(
            "注释行，不参与路径匹配。使用 `\\#` 可匹配以 `#` 开头的名称。",
            "Comment line; it does not match paths. Use `\\#` to match a name beginning with `#`.",
        );
    }

    let (effective, ignored_trailing_spaces) = trim_unescaped_trailing_spaces(line);
    let negated = effective.starts_with('!');
    let escaped_leading_marker = effective.starts_with("\\!") || effective.starts_with("\\#");
    let mut pattern = effective;
    if negated {
        pattern = &pattern[1..];
    }
    if pattern.is_empty() {
        return explanation(
            "空的否定规则，不匹配任何路径。",
            "Empty negation rule; it does not match any path.",
        );
    }
    if trailing_backslash_run(pattern) % 2 == 1 {
        return explanation(
            "规则以未配对的反斜杠结尾，是无效规则，不会匹配任何路径。",
            "The pattern ends with an unmatched backslash, so it is invalid and never matches any path.",
        );
    }

    let leading_slash = pattern.starts_with('/');
    let directory_only = pattern.ends_with('/') && !is_escaped(pattern, pattern.len() - 1);
    let core = pattern
        .strip_prefix('/')
        .unwrap_or(pattern)
        .strip_suffix('/')
        .unwrap_or_else(|| pattern.strip_prefix('/').unwrap_or(pattern));
    if core.is_empty() {
        return explanation(
            "规则不包含可匹配的名称，不会匹配任何路径。",
            "The pattern contains no matchable name and does not match any path.",
        );
    }
    let contains_slash = core.contains('/');
    let literal = !contains_unescaped_wildcard(core);
    let display = unescape_for_display(core);

    let action_zh = if negated {
        "尝试重新包含"
    } else {
        "忽略"
    };
    let action_en = if negated { "Re-include" } else { "Ignore" };
    let target_zh = if directory_only {
        "目录"
    } else {
        "文件或目录"
    };
    let target_en = if directory_only {
        "directories"
    } else {
        "files or directories"
    };

    let mut zh = if literal {
        if leading_slash || contains_slash {
            format!(
                "{action_zh}相对于此 `.gitignore` 所在目录、路径恰好为“{display}”的{target_zh}。"
            )
        } else {
            format!(
                "{action_zh}此 `.gitignore` 所在目录及任意子目录中，名称恰好为“{display}”的{target_zh}。"
            )
        }
    } else if leading_slash || contains_slash {
        format!("{action_zh}相对于此 `.gitignore` 所在目录、匹配“{display}”的{target_zh}。")
    } else {
        format!(
            "{action_zh}此 `.gitignore` 所在目录及任意子目录中，名称匹配“{display}”的{target_zh}。"
        )
    };
    let mut en = if literal {
        if leading_slash || contains_slash {
            format!(
                "{action_en} {target_en} whose path relative to this `.gitignore` directory is exactly `{display}`."
            )
        } else {
            format!(
                "{action_en} {target_en} named exactly `{display}` in this `.gitignore` directory or any descendant."
            )
        }
    } else if leading_slash || contains_slash {
        format!(
            "{action_en} {target_en} whose path relative to this `.gitignore` directory matches `{display}`."
        )
    } else {
        format!(
            "{action_en} {target_en} whose name matches `{display}` in this `.gitignore` directory or any descendant."
        )
    };

    if core.starts_with("**/") {
        push_note(
            &mut zh,
            "开头的 `**/` 可跨越任意层级目录（包括零层）。",
            &mut en,
            "Leading `**/` crosses any number of directory levels, including zero.",
        );
    }
    if core.ends_with("/**") {
        push_note(
            &mut zh,
            "结尾的 `/**` 匹配该目录内部的全部内容，层级不限。",
            &mut en,
            "Trailing `/**` matches everything inside that directory at unlimited depth.",
        );
    }
    if core.contains("/**/") {
        push_note(
            &mut zh,
            "中间的 `/**/` 匹配零个或多个目录层级。",
            &mut en,
            "Middle `/**/` matches zero or more directory levels.",
        );
    }
    if contains_ordinary_star(core) {
        push_note(
            &mut zh,
            "`*` 匹配除 `/` 外的任意数量字符。",
            &mut en,
            "`*` matches any number of characters except `/`.",
        );
    }
    if contains_unescaped(core, '?') {
        push_note(
            &mut zh,
            "`?` 匹配除 `/` 外的单个字符。",
            &mut en,
            "`?` matches one character except `/`.",
        );
    }
    if contains_character_class(core) {
        push_note(
            &mut zh,
            "`[...]` 匹配一个列出的字符或范围；首字符 `!`/`^` 表示排除该集合。",
            &mut en,
            "`[...]` matches one listed character or range; leading `!`/`^` negates the class.",
        );
    }
    if escaped_leading_marker {
        push_note(
            &mut zh,
            "开头的反斜杠把 `#` 或 `!` 转义为普通字符。",
            &mut en,
            "The leading backslash escapes `#` or `!` as a literal character.",
        );
    }
    if ignored_trailing_spaces {
        push_note(
            &mut zh,
            "末尾未转义空格会被忽略；使用 `\\ ` 可匹配字面空格。",
            &mut en,
            "Unescaped trailing spaces are ignored; use `\\ ` to match a literal space.",
        );
    }
    if negated {
        push_note(
            &mut zh,
            "若其父目录已被忽略，Git 不会继续遍历，单靠此规则无法重新包含该路径。",
            &mut en,
            "If its parent directory is ignored, Git does not traverse it, so this rule alone cannot re-include the path.",
        );
    }
    explanation(zh, en)
}

fn explanation(zh: impl Into<String>, en: impl Into<String>) -> GitignoreExplanation {
    GitignoreExplanation {
        zh: zh.into(),
        en: en.into(),
    }
}

fn push_note(zh: &mut String, zh_note: &str, en: &mut String, en_note: &str) {
    zh.push_str(zh_note);
    en.push(' ');
    en.push_str(en_note);
}

fn trim_unescaped_trailing_spaces(value: &str) -> (&str, bool) {
    let mut end = value.len();
    let mut removed = false;
    while end > 0 && value.as_bytes()[end - 1] == b' ' && !is_escaped(value, end - 1) {
        end -= 1;
        removed = true;
    }
    (&value[..end], removed)
}

fn is_escaped(value: &str, index: usize) -> bool {
    let preceding_backslashes = value[..index]
        .as_bytes()
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count();
    preceding_backslashes % 2 == 1
}

fn contains_unescaped(value: &str, needle: char) -> bool {
    value
        .char_indices()
        .any(|(index, character)| character == needle && !is_escaped(value, index))
}

fn contains_unescaped_wildcard(value: &str) -> bool {
    contains_unescaped(value, '*')
        || contains_unescaped(value, '?')
        || contains_character_class(value)
}

fn contains_ordinary_star(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'*' || is_escaped(value, index) {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len() && bytes[index] == b'*' {
            index += 1;
        }
        let run_len = index - start;
        let valid_double_star = run_len == 2
            && ((start == 0 && bytes.get(index) == Some(&b'/'))
                || (start > 0 && bytes[start - 1] == b'/' && index == bytes.len())
                || (start > 0 && bytes[start - 1] == b'/' && bytes.get(index) == Some(&b'/')));
        if !valid_double_star {
            return true;
        }
    }
    false
}

fn trailing_backslash_run(value: &str) -> usize {
    value
        .as_bytes()
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
}

fn contains_character_class(value: &str) -> bool {
    value.char_indices().any(|(open, character)| {
        character == '['
            && !is_escaped(value, open)
            && value[open + 1..]
                .char_indices()
                .any(|(offset, close)| close == ']' && !is_escaped(value, open + 1 + offset))
    })
}

fn unescape_for_display(value: &str) -> String {
    let mut output = String::new();
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            output.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            output.push(character);
        }
    }
    if escaped {
        output.push('\\');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_round_trips_line_endings_and_final_newline() {
        for text in ["", "a.txt", "a.txt\n", "a.txt\r\n!keep.txt\r\n", "\n"] {
            assert_eq!(GitignoreDocument::from_text(text).to_text(), text);
        }
    }

    #[test]
    fn exact_basename_is_explained_at_every_depth() {
        let explanation = explain_gitignore_line("a.txt");
        assert!(explanation.zh.contains("名称恰好为“a.txt”"));
        assert!(explanation.zh.contains("任意子目录"));
        assert!(explanation.en.contains("named exactly `a.txt`"));
        assert!(explanation.en.contains("any descendant"));
    }

    #[test]
    fn explains_comments_negation_anchoring_and_directory_rules() {
        assert!(explain_gitignore_line("# note").en.contains("Comment"));
        assert!(
            explain_gitignore_line("!keep.log")
                .en
                .contains("Re-include")
        );
        assert!(
            explain_gitignore_line("!keep.log")
                .en
                .contains("parent directory")
        );
        assert!(explain_gitignore_line("/build").en.contains("relative"));
        assert!(explain_gitignore_line("cache/").en.contains("directories"));
        assert!(explain_gitignore_line("doc/output").en.contains("relative"));
    }

    #[test]
    fn explains_all_gitignore_wildcard_forms_and_escapes() {
        assert!(explain_gitignore_line("*.log").en.contains("except `/`"));
        assert!(
            explain_gitignore_line("file?.txt")
                .en
                .contains("one character")
        );
        assert!(explain_gitignore_line("file[0-9].txt").en.contains("range"));
        assert!(
            explain_gitignore_line("**/temp")
                .en
                .contains("including zero")
        );
        assert!(
            explain_gitignore_line("logs/**")
                .en
                .contains("unlimited depth")
        );
        assert!(explain_gitignore_line("a/**/b").en.contains("zero or more"));
        assert!(
            explain_gitignore_line("\\#literal")
                .en
                .contains("literal character")
        );
        assert!(
            explain_gitignore_line("name  ")
                .en
                .contains("trailing spaces")
        );
        assert!(
            !explain_gitignore_line("name\\ ")
                .en
                .contains("trailing spaces")
        );
        assert!(
            explain_gitignore_line("a/**/b*.txt")
                .en
                .contains("`*` matches")
        );
        assert!(explain_gitignore_line("ab**cd").en.contains("`*` matches"));
        assert!(explain_gitignore_line("invalid\\").en.contains("invalid"));
        assert!(explain_gitignore_line("/").en.contains("no matchable name"));
    }

    #[test]
    fn save_refuses_to_overwrite_an_external_change() {
        let root = std::env::temp_dir().join(format!(
            "git-agent-gitignore-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(".gitignore"), "old\n").unwrap();
        let mut document = GitignoreDocument::from_text("old\n");
        document.lines[0] = "new".to_owned();
        fs::write(root.join(".gitignore"), "external\n").unwrap();

        let error = save_repository_gitignore(&root, "old\n", &document).unwrap_err();
        assert!(error.to_string().contains("changed on disk"));
        assert_eq!(
            fs::read_to_string(root.join(".gitignore")).unwrap(),
            "external\n"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
