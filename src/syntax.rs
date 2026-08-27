use std::{
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use oxc_resolver::{ResolveOptions, Resolver, TsconfigDiscovery};
use serde::{Deserialize, Serialize};
use syntect::{
    easy::ScopeRangeIterator,
    parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet},
};

use crate::diagnostics;

pub const SYNTAX_PLUGIN_API_VERSION: u32 = 1;
const MAX_HIGHLIGHT_BYTES: usize = 1024 * 1024;
const MAX_HIGHLIGHT_LINES: usize = 20_000;
const MAX_HIGHLIGHT_LINE_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SyntaxRole {
    #[default]
    Plain,
    Comment,
    String,
    Number,
    Keyword,
    Type,
    Function,
    Constant,
    Variable,
    Tag,
    Attribute,
    Operator,
    Punctuation,
    Invalid,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct HighlightSpan {
    pub start: usize,
    pub end: usize,
    pub role: SyntaxRole,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct HighlightedLine {
    pub spans: Vec<HighlightSpan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Framework {
    React,
    Vue,
    Preact,
    Solid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DetectionConfidence {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SyntaxDetection {
    pub language: String,
    #[serde(default)]
    pub framework: Option<Framework>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub confidence: Option<DetectionConfidence>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct HighlightedDocument {
    pub detection: SyntaxDetection,
    pub lines: Vec<HighlightedLine>,
}

impl HighlightedDocument {
    pub fn line(&self, one_based: &str) -> Option<&HighlightedLine> {
        let index = one_based.parse::<usize>().ok()?.checked_sub(1)?;
        self.lines.get(index)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct SyntaxPluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: u32,
    #[serde(default = "default_syntaxes_dir")]
    pub syntaxes_dir: String,
}

fn default_syntaxes_dir() -> String {
    "syntaxes".to_owned()
}

pub fn syntax_plugin_root() -> Option<PathBuf> {
    if cfg!(test)
        && let Some(path) = std::env::var_os("GIT_AGENT_SYNTAX_PLUGIN_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    {
        return Some(path);
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .map(|base| base.join("data").join("plugins").join("syntax"))
}

pub fn highlight_document(
    repository_root: &Path,
    path: &str,
    source: &str,
) -> Option<HighlightedDocument> {
    if source.len() > MAX_HIGHLIGHT_BYTES {
        return None;
    }

    let lines = source.split_inclusive('\n').collect::<Vec<_>>();
    if lines.len() > MAX_HIGHLIGHT_LINES
        || lines
            .iter()
            .any(|line| line.len() > MAX_HIGHLIGHT_LINE_BYTES)
    {
        return None;
    }

    let syntax_set = syntax_set();
    let syntax = syntax_for_path(syntax_set, path)?;
    let detection = detect_syntax_context(repository_root, path, source, syntax);
    let mut parser = ParseState::new(syntax);
    let mut scope_stack = ScopeStack::new();
    let mut highlighted_lines = Vec::with_capacity(lines.len().max(1));
    let mut vue_semantic_state = (syntax.name == "Vue Component").then(VueSemanticState::default);

    for raw_line in lines {
        let visible_line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let visible_len = visible_line
            .strip_suffix('\r')
            .unwrap_or(visible_line)
            .len();
        let operations = parser.parse_line(raw_line, syntax_set).ok()?;
        let mut spans = Vec::<HighlightSpan>::new();

        for (range, operation) in ScopeRangeIterator::new(&operations, raw_line) {
            if scope_stack.apply(operation).is_err() {
                return None;
            }
            let start = range.start.min(visible_len);
            let end = range.end.min(visible_len);
            if start >= end {
                continue;
            }
            let role = role_for_scope_stack(&scope_stack);
            if role == SyntaxRole::Plain {
                continue;
            }
            if let Some(previous) = spans.last_mut()
                && previous.end == start
                && previous.role == role
            {
                previous.end = end;
                continue;
            }
            spans.push(HighlightSpan { start, end, role });
        }
        if let Some(state) = vue_semantic_state.as_mut() {
            enrich_vue_semantic_roles(&visible_line[..visible_len], &mut spans, state);
        }
        highlighted_lines.push(HighlightedLine { spans });
    }

    if source.is_empty() {
        highlighted_lines.push(HighlightedLine::default());
    }

    Some(HighlightedDocument {
        detection,
        lines: highlighted_lines,
    })
}

const VUE_COMPILER_MACROS: &[&str] = &[
    "defineEmits",
    "defineExpose",
    "defineModel",
    "defineOptions",
    "defineProps",
    "defineSlots",
    "withDefaults",
];

#[derive(Debug, Default)]
struct VueSemanticState {
    in_script: bool,
    reading_script_open_tag: bool,
    script_setup: bool,
}

fn enrich_vue_semantic_roles(
    line: &str,
    spans: &mut Vec<HighlightSpan>,
    state: &mut VueSemanticState,
) {
    let lowercase = line.to_ascii_lowercase();
    let code_start = if state.in_script {
        0
    } else {
        let fragment_start = if state.reading_script_open_tag {
            0
        } else {
            let Some(start) = vue_script_open_tag_start(&lowercase) else {
                return;
            };
            state.reading_script_open_tag = true;
            state.script_setup = false;
            start
        };
        let fragment = &line[fragment_start..];
        if vue_tag_fragment_has_setup_attribute(fragment) {
            state.script_setup = true;
        }
        let Some(tag_end) = vue_tag_end(fragment) else {
            return;
        };
        state.reading_script_open_tag = false;
        state.in_script = true;
        fragment_start + tag_end + 1
    };

    let remaining = &lowercase[code_start..];
    let code_end = code_start + remaining.find("</script").unwrap_or(remaining.len());
    enrich_vue_script_code(
        &line[code_start..code_end],
        code_start,
        spans,
        state.script_setup,
    );

    if code_end < line.len() {
        state.in_script = false;
        state.reading_script_open_tag = false;
        state.script_setup = false;
    }
}

fn vue_script_open_tag_start(lowercase: &str) -> Option<usize> {
    lowercase.match_indices("<script").find_map(|(start, tag)| {
        let after = lowercase.as_bytes().get(start + tag.len()).copied();
        after
            .is_none_or(|byte| byte.is_ascii_whitespace() || matches!(byte, b'>' | b'/'))
            .then_some(start)
    })
}

fn vue_tag_end(fragment: &str) -> Option<usize> {
    let mut quote = None;
    for (index, byte) in fragment.bytes().enumerate() {
        match (quote, byte) {
            (Some(expected), current) if current == expected => quote = None,
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, b'>') => return Some(index),
            _ => {}
        }
    }
    None
}

fn vue_tag_fragment_has_setup_attribute(fragment: &str) -> bool {
    let lowercase = fragment.to_ascii_lowercase();
    let bytes = lowercase.as_bytes();
    let mut quote = None;
    let mut index = 0;
    while index < bytes.len() {
        match (quote, bytes[index]) {
            (Some(expected), current) if current == expected => quote = None,
            (Some(_), _) => {}
            (None, b'\'' | b'"') => quote = Some(bytes[index]),
            (None, _) if bytes[index..].starts_with(b"setup") => {
                let before_is_boundary = index == 0 || bytes[index - 1].is_ascii_whitespace();
                let after = bytes.get(index + b"setup".len()).copied();
                let after_is_boundary = after.is_none_or(|byte| {
                    byte.is_ascii_whitespace() || matches!(byte, b'=' | b'>' | b'/')
                });
                if before_is_boundary && after_is_boundary {
                    return true;
                }
            }
            _ => {}
        }
        index += 1;
    }
    false
}

fn enrich_vue_script_code(
    code: &str,
    line_offset: usize,
    spans: &mut Vec<HighlightSpan>,
    script_setup: bool,
) {
    for (start, end) in javascript_identifier_ranges(code) {
        let identifier = &code[start..end];
        let compiler_macro = script_setup && VUE_COMPILER_MACROS.contains(&identifier);
        let call_site = code[end..].trim_start().starts_with('(')
            && !matches!(
                identifier,
                "catch" | "for" | "if" | "switch" | "while" | "with"
            );
        if compiler_macro || call_site {
            overlay_syntax_role(
                spans,
                line_offset + start,
                line_offset + end,
                SyntaxRole::Function,
            );
        }
    }
}

fn javascript_identifier_ranges(line: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = None;
    for (index, character) in line.char_indices() {
        let continues = character.is_ascii_alphanumeric() || matches!(character, '_' | '$');
        if start.is_some() && continues {
            continue;
        }
        if let Some(identifier_start) = start.take() {
            ranges.push((identifier_start, index));
        }
        if character.is_ascii_alphabetic() || matches!(character, '_' | '$') {
            start = Some(index);
        }
    }
    if let Some(identifier_start) = start {
        ranges.push((identifier_start, line.len()));
    }
    ranges
}

fn overlay_syntax_role(spans: &mut Vec<HighlightSpan>, start: usize, end: usize, role: SyntaxRole) {
    if spans.iter().any(|span| {
        span.start < end
            && span.end > start
            && matches!(
                span.role,
                SyntaxRole::Comment | SyntaxRole::String | SyntaxRole::Invalid
            )
    }) {
        return;
    }

    let mut updated = Vec::with_capacity(spans.len() + 1);
    for span in spans.drain(..) {
        if span.end <= start || span.start >= end {
            updated.push(span);
            continue;
        }
        if span.start < start {
            updated.push(HighlightSpan {
                start: span.start,
                end: start,
                role: span.role,
            });
        }
        if span.end > end {
            updated.push(HighlightSpan {
                start: end,
                end: span.end,
                role: span.role,
            });
        }
    }
    updated.push(HighlightSpan { start, end, role });
    updated.sort_by_key(|span| span.start);

    let mut merged = Vec::<HighlightSpan>::with_capacity(updated.len());
    for span in updated {
        if let Some(previous) = merged.last_mut()
            && previous.end == span.start
            && previous.role == span.role
        {
            previous.end = span.end;
        } else {
            merged.push(span);
        }
    }
    *spans = merged;
}

fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX_SET.get_or_init(build_syntax_set)
}

fn build_syntax_set() -> SyntaxSet {
    build_syntax_set_from_root(syntax_plugin_root().as_deref())
}

fn build_syntax_set_from_root(root: Option<&Path>) -> SyntaxSet {
    let mut builder = two_face::syntax::extra_newlines().into_builder();
    if let Some(root) = root {
        for plugin_dir in child_directories(root) {
            let manifest_path = plugin_dir.join("plugin.json");
            let Ok(source) = fs::read_to_string(&manifest_path) else {
                continue;
            };
            let Ok(manifest) = serde_json::from_str::<SyntaxPluginManifest>(&source) else {
                diagnostics::app_error(
                    "syntax.plugin.invalid_manifest",
                    &format!("path={}", manifest_path.display()),
                );
                continue;
            };
            if manifest.api_version != SYNTAX_PLUGIN_API_VERSION
                || manifest.id.trim().is_empty()
                || manifest.name.trim().is_empty()
                || manifest.version.trim().is_empty()
                || !safe_plugin_relative_path(Path::new(&manifest.syntaxes_dir))
            {
                diagnostics::app_error(
                    "syntax.plugin.incompatible",
                    &format!(
                        "path={} id={} api={}",
                        manifest_path.display(),
                        manifest.id,
                        manifest.api_version
                    ),
                );
                continue;
            }
            let syntax_dir = plugin_dir.join(&manifest.syntaxes_dir);
            match builder.add_from_folder(&syntax_dir, true) {
                Ok(()) => diagnostics::app_info(
                    "syntax.plugin.loaded",
                    &format!("id={} version={}", manifest.id, manifest.version),
                ),
                Err(error) => diagnostics::app_error(
                    "syntax.plugin.load_failed",
                    &format!("id={} error={error}", manifest.id),
                ),
            }
        }
    }
    builder.build()
}

fn child_directories(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut directories = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    directories.sort();
    directories
}

fn safe_plugin_relative_path(path: &Path) -> bool {
    let Some(path) = path.to_str() else {
        return false;
    };
    let normalized = path.replace('\\', "/");
    let has_windows_drive_prefix = normalized.as_bytes().get(1) == Some(&b':')
        && normalized
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic);

    !normalized.is_empty()
        && !normalized.starts_with('/')
        && !has_windows_drive_prefix
        && normalized.split('/').all(|component| component != "..")
}

fn syntax_for_path<'a>(syntax_set: &'a SyntaxSet, path: &str) -> Option<&'a SyntaxReference> {
    let normalized = path.replace('\\', "/");
    let filename = normalized.rsplit('/').next().unwrap_or(normalized.as_str());
    let lowercase = filename.to_ascii_lowercase();
    let preferred_name = match lowercase.as_str() {
        "dockerfile" | "containerfile" => Some("Dockerfile"),
        "makefile" | "gnumakefile" => Some("Makefile"),
        ".env" => Some("DotENV"),
        ".gitignore" | ".gitattributes" | ".gitmodules" => Some("Git Config"),
        _ if lowercase.ends_with(".jsx") => Some("JavaScript (Babel)"),
        _ if lowercase.ends_with(".tsx") => Some("TypeScriptReact"),
        _ if lowercase.ends_with(".vue") => Some("Vue Component"),
        _ => None,
    };
    if let Some(syntax) = preferred_name.and_then(|name| syntax_set.find_syntax_by_name(name)) {
        return Some(syntax);
    }
    let extension = Path::new(filename).extension()?.to_str()?;
    syntax_set.find_syntax_by_extension(extension)
}

fn detect_syntax_context(
    repository_root: &Path,
    path: &str,
    source: &str,
    syntax: &SyntaxReference,
) -> SyntaxDetection {
    let mut detection = SyntaxDetection {
        language: syntax.name.clone(),
        ..Default::default()
    };
    let extension = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    if !matches!(extension.to_ascii_lowercase().as_str(), "jsx" | "tsx") {
        return detection;
    }

    if let Some(import_source) = jsx_import_source_pragma(source) {
        detection.framework = framework_from_import_source(&import_source);
        detection.source = Some("file pragma".to_owned());
        detection.confidence = detection.framework.map(|_| DetectionConfidence::High);
        if detection.framework.is_some() {
            return detection;
        }
    }

    let absolute_path = repository_root.join(path);
    if let Some((framework, config_path, confidence)) = framework_from_tsconfig(&absolute_path) {
        detection.framework = Some(framework);
        detection.source = Some(config_path.display().to_string());
        detection.confidence = Some(confidence);
        return detection;
    }

    if let Some(framework) = framework_from_imports(source) {
        detection.framework = Some(framework);
        detection.source = Some("source imports".to_owned());
        detection.confidence = Some(DetectionConfidence::Medium);
        return detection;
    }

    if let Some((framework, package_path)) = framework_from_nearest_package(repository_root, path) {
        detection.framework = Some(framework);
        detection.source = Some(package_path.display().to_string());
        detection.confidence = Some(DetectionConfidence::Medium);
    }
    detection
}

fn jsx_import_source_pragma(source: &str) -> Option<String> {
    let prefix = source.get(..source.len().min(4096)).unwrap_or(source);
    let marker = "@jsxImportSource";
    let tail = prefix.split_once(marker)?.1.trim_start();
    let value = tail
        .split(|ch: char| ch.is_whitespace() || matches!(ch, '*' | '/'))
        .next()
        .unwrap_or_default()
        .trim_matches(['\'', '"']);
    (!value.is_empty()).then(|| value.to_owned())
}

fn tsconfig_resolver() -> &'static Resolver {
    static RESOLVER: OnceLock<Resolver> = OnceLock::new();
    RESOLVER.get_or_init(|| {
        Resolver::new(ResolveOptions {
            tsconfig: Some(TsconfigDiscovery::Auto),
            ..ResolveOptions::default()
        })
    })
}

fn framework_from_tsconfig(path: &Path) -> Option<(Framework, PathBuf, DetectionConfidence)> {
    let config = tsconfig_resolver().find_tsconfig(path).ok()??;
    let options = &config.compiler_options;
    if let Some(framework) = options
        .jsx_import_source
        .as_deref()
        .and_then(framework_from_import_source)
    {
        return Some((framework, config.path.clone(), DetectionConfidence::High));
    }
    let factory = options.jsx_factory.as_deref().unwrap_or_default();
    let framework = if factory.starts_with("React.") {
        Some(Framework::React)
    } else if factory.starts_with("Vue.") {
        Some(Framework::Vue)
    } else {
        None
    };
    if let Some(framework) = framework {
        return Some((framework, config.path.clone(), DetectionConfidence::Medium));
    }
    match options
        .jsx
        .as_deref()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("react" | "react-jsx" | "react-jsxdev") => Some((
            Framework::React,
            config.path.clone(),
            DetectionConfidence::Medium,
        )),
        _ => None,
    }
}

fn framework_from_import_source(value: &str) -> Option<Framework> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized == "vue" || normalized.starts_with("vue/") {
        Some(Framework::Vue)
    } else if normalized == "preact" || normalized.starts_with("preact/") {
        Some(Framework::Preact)
    } else if normalized == "solid-js" || normalized.starts_with("solid-js/") {
        Some(Framework::Solid)
    } else if normalized == "react" || normalized.starts_with("react/") {
        Some(Framework::React)
    } else {
        None
    }
}

fn framework_from_imports(source: &str) -> Option<Framework> {
    let prefix = source.get(..source.len().min(32 * 1024)).unwrap_or(source);
    [
        (Framework::Vue, ["from 'vue'", "from \"vue\""]),
        (Framework::Preact, ["from 'preact", "from \"preact"]),
        (Framework::Solid, ["from 'solid-js", "from \"solid-js"]),
        (Framework::React, ["from 'react", "from \"react"]),
    ]
    .into_iter()
    .find_map(|(framework, needles)| {
        needles
            .iter()
            .any(|needle| prefix.contains(needle))
            .then_some(framework)
    })
}

fn framework_from_nearest_package(
    repository_root: &Path,
    path: &str,
) -> Option<(Framework, PathBuf)> {
    let mut directory = repository_root.join(path).parent()?.to_path_buf();
    loop {
        let package_path = directory.join("package.json");
        if let Ok(source) = fs::read_to_string(&package_path)
            && let Ok(package) = serde_json::from_str::<serde_json::Value>(&source)
        {
            let mut matches = Vec::new();
            for section in ["dependencies", "devDependencies", "peerDependencies"] {
                let Some(dependencies) = package.get(section).and_then(|value| value.as_object())
                else {
                    continue;
                };
                for (name, framework) in [
                    ("vue", Framework::Vue),
                    ("preact", Framework::Preact),
                    ("solid-js", Framework::Solid),
                    ("react", Framework::React),
                ] {
                    if dependencies.contains_key(name) && !matches.contains(&framework) {
                        matches.push(framework);
                    }
                }
            }
            if matches.len() == 1 {
                return Some((matches[0], package_path));
            }
            if matches.len() > 1 {
                return None;
            }
        }
        if directory == repository_root || !directory.pop() {
            break;
        }
    }
    None
}

fn role_for_scope_stack(stack: &ScopeStack) -> SyntaxRole {
    for scope in stack.scopes.iter().rev() {
        for (prefix, role) in scope_roles() {
            if prefix.is_prefix_of(*scope) {
                return *role;
            }
        }
    }
    SyntaxRole::Plain
}

fn scope_roles() -> &'static Vec<(Scope, SyntaxRole)> {
    static SCOPE_ROLES: OnceLock<Vec<(Scope, SyntaxRole)>> = OnceLock::new();
    SCOPE_ROLES.get_or_init(|| {
        [
            ("invalid", SyntaxRole::Invalid),
            ("comment", SyntaxRole::Comment),
            ("string", SyntaxRole::String),
            ("constant.numeric", SyntaxRole::Number),
            ("constant.language", SyntaxRole::Constant),
            ("constant.character", SyntaxRole::Constant),
            ("keyword.operator", SyntaxRole::Operator),
            ("keyword", SyntaxRole::Keyword),
            ("storage", SyntaxRole::Keyword),
            ("entity.name.type", SyntaxRole::Type),
            ("entity.name.class", SyntaxRole::Type),
            ("support.type", SyntaxRole::Type),
            ("support.class", SyntaxRole::Type),
            ("entity.name.function", SyntaxRole::Function),
            ("support.function", SyntaxRole::Function),
            ("entity.name.tag", SyntaxRole::Tag),
            ("entity.other.attribute-name", SyntaxRole::Attribute),
            ("variable.parameter", SyntaxRole::Variable),
            ("variable", SyntaxRole::Variable),
            ("constant", SyntaxRole::Constant),
            ("punctuation", SyntaxRole::Punctuation),
        ]
        .into_iter()
        .filter_map(|(scope, role)| Scope::new(scope).ok().map(|scope| (scope, role)))
        .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_bundled_frontend_syntaxes() {
        let set = syntax_set();
        assert_eq!(
            syntax_for_path(set, "src/view.jsx").unwrap().name,
            "JavaScript (Babel)"
        );
        assert_eq!(
            syntax_for_path(set, "src/view.tsx").unwrap().name,
            "TypeScriptReact"
        );
        assert_eq!(
            syntax_for_path(set, "src/View.vue").unwrap().name,
            "Vue Component"
        );
    }

    #[test]
    fn file_pragma_selects_vue_for_tsx() {
        let root = Path::new("C:/nonexistent-repository");
        let document = highlight_document(
            root,
            "src/view.tsx",
            "/** @jsxImportSource vue */\nexport const View = () => <main>ok</main>;\n",
        )
        .unwrap();
        assert_eq!(document.detection.framework, Some(Framework::Vue));
        assert_eq!(
            document.detection.confidence,
            Some(DetectionConfidence::High)
        );
    }

    #[test]
    fn project_reference_selects_the_tsconfig_that_owns_tsx_file() {
        let root = std::env::temp_dir().join(format!(
            "git-agent-tsconfig-detection-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("app/src")).unwrap();
        fs::write(
            root.join("tsconfig.json"),
            r#"{"files":[],"references":[{"path":"./app/tsconfig.app.json"}]}"#,
        )
        .unwrap();
        fs::write(
            root.join("app/tsconfig.app.json"),
            r#"{"compilerOptions":{"jsx":"preserve","jsxImportSource":"vue"},"include":["src"]}"#,
        )
        .unwrap();
        let source = "export const View = () => <main>ok</main>;\n";
        fs::write(root.join("app/src/View.tsx"), source).unwrap();

        let document = highlight_document(&root, "app/src/View.tsx", source).unwrap();
        assert_eq!(document.detection.framework, Some(Framework::Vue));
        assert!(
            document
                .detection
                .source
                .as_deref()
                .is_some_and(|source| source.ends_with("tsconfig.app.json"))
        );
        assert_eq!(
            document.detection.confidence,
            Some(DetectionConfidence::High)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parser_keeps_multiline_comment_state() {
        let document = highlight_document(
            Path::new("C:/nonexistent-repository"),
            "src/lib.ts",
            "/* comment starts\nstill a comment */\nconst value = 1;\n",
        )
        .unwrap();
        assert!(
            document.lines[1]
                .spans
                .iter()
                .any(|span| span.role == SyntaxRole::Comment)
        );
        assert!(
            document.lines[2]
                .spans
                .iter()
                .any(|span| span.role == SyntaxRole::Keyword)
        );
    }

    #[test]
    fn vue_arrow_function_highlights_the_complete_operator() {
        let vue_source = "<script setup lang=\"tsx\">\nconst column = { default: ({ row }) => (\n  <div class=\"box\">{row.name}</div>\n) }\n</script>\n";
        let vue = highlight_document(
            Path::new("C:/nonexistent-repository"),
            "src/View.vue",
            vue_source,
        )
        .unwrap();
        let line = vue_source.lines().nth(1).unwrap();
        let start = line.find("=>").unwrap();
        assert!(
            vue.lines[1]
                .spans
                .iter()
                .any(|span| span.start <= start && span.end >= start + 2)
        );
    }

    #[test]
    fn vue_compiler_macros_and_call_sites_have_function_roles() {
        let source = "<script setup lang=\"ts\">\nconst props = withDefaults(\n  defineProps<{ value?: string }>(),\n  {},\n)\nconst doubled = computed(() => props.value)\nconst literal = 'withDefaults('\n// defineProps<{ ignored: true }>()\n</script>\n";
        let document = highlight_document(
            Path::new("C:/nonexistent-repository"),
            "src/View.vue",
            source,
        )
        .unwrap();
        let has_role = |line_index: usize, token: &str, role: SyntaxRole| {
            let line = source.lines().nth(line_index).unwrap();
            let start = line.find(token).unwrap();
            document.lines[line_index].spans.iter().any(|span| {
                span.start <= start && span.end >= start + token.len() && span.role == role
            })
        };

        assert!(has_role(1, "const", SyntaxRole::Keyword));
        assert!(has_role(1, "withDefaults", SyntaxRole::Function));
        assert!(has_role(2, "defineProps", SyntaxRole::Function));
        assert!(has_role(5, "computed", SyntaxRole::Function));
        assert!(!has_role(6, "withDefaults", SyntaxRole::Function));
        assert!(!has_role(7, "defineProps", SyntaxRole::Function));
    }

    #[test]
    fn ordinary_vue_script_does_not_assume_vue_three_compiler_macros() {
        let mut state = VueSemanticState::default();
        enrich_vue_semantic_roles("<script lang=\"ts\">", &mut Vec::new(), &mut state);

        let macro_line = "const props = defineProps<Props>";
        let mut macro_spans = Vec::new();
        enrich_vue_semantic_roles(macro_line, &mut macro_spans, &mut state);
        let macro_start = macro_line.find("defineProps").unwrap();
        assert!(!macro_spans.iter().any(|span| {
            span.start <= macro_start
                && span.end >= macro_start + "defineProps".len()
                && span.role == SyntaxRole::Function
        }));

        let call_line = "const result = transform(props)";
        let mut call_spans = Vec::new();
        enrich_vue_semantic_roles(call_line, &mut call_spans, &mut state);
        let call_start = call_line.find("transform").unwrap();
        assert!(call_spans.iter().any(|span| {
            span.start <= call_start
                && span.end >= call_start + "transform".len()
                && span.role == SyntaxRole::Function
        }));
    }

    #[test]
    fn multiline_script_setup_enables_vue_three_compiler_macros() {
        let mut state = VueSemanticState::default();
        for line in ["<script", "  lang=\"ts\"", "  setup", ">"] {
            enrich_vue_semantic_roles(line, &mut Vec::new(), &mut state);
        }

        let macro_line = "const props = defineProps<Props>";
        let mut spans = Vec::new();
        enrich_vue_semantic_roles(macro_line, &mut spans, &mut state);
        let start = macro_line.find("defineProps").unwrap();
        assert!(spans.iter().any(|span| {
            span.start <= start
                && span.end >= start + "defineProps".len()
                && span.role == SyntaxRole::Function
        }));

        let mut ordinary_state = VueSemanticState::default();
        enrich_vue_semantic_roles(
            "<script data-mode=\"setup\">",
            &mut Vec::new(),
            &mut ordinary_state,
        );
        let mut ordinary_spans = Vec::new();
        enrich_vue_semantic_roles(macro_line, &mut ordinary_spans, &mut ordinary_state);
        assert!(!ordinary_spans.iter().any(|span| {
            span.start <= start
                && span.end >= start + "defineProps".len()
                && span.role == SyntaxRole::Function
        }));
    }

    #[test]
    fn oversized_input_falls_back_to_plain_text() {
        let source = "x".repeat(MAX_HIGHLIGHT_BYTES + 1);
        assert!(highlight_document(Path::new("."), "large.rs", &source).is_none());
    }

    #[test]
    fn manifest_defaults_to_syntaxes_folder() {
        let manifest: SyntaxPluginManifest = serde_json::from_str(
            r#"{"id":"example","name":"Example","version":"1.0.0","api_version":1}"#,
        )
        .unwrap();
        assert_eq!(manifest.syntaxes_dir, "syntaxes");
    }

    #[test]
    fn loads_external_data_only_syntax_plugin() {
        let root =
            std::env::temp_dir().join(format!("git-agent-syntax-plugin-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let plugin = root.join("example-language");
        fs::create_dir_all(plugin.join("syntaxes")).unwrap();
        fs::write(
            plugin.join("plugin.json"),
            r#"{"id":"example-language","name":"Example Language","version":"1.0.0","api_version":1}"#,
        )
        .unwrap();
        fs::write(
            plugin.join("syntaxes").join("Example.sublime-syntax"),
            "%YAML 1.2\n---\nname: Example Language\nfile_extensions: [example]\nscope: source.example\ncontexts:\n  main:\n    - match: '\\bexample\\b'\n      scope: keyword.control.example\n",
        )
        .unwrap();

        let set = build_syntax_set_from_root(Some(&root));
        assert_eq!(
            set.find_syntax_by_extension("example").unwrap().name,
            "Example Language"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plugin_syntax_directory_cannot_escape_plugin_root() {
        assert!(safe_plugin_relative_path(Path::new("syntaxes")));
        assert!(safe_plugin_relative_path(Path::new("./syntaxes")));
        assert!(!safe_plugin_relative_path(Path::new("../syntaxes")));
        assert!(!safe_plugin_relative_path(Path::new("..\\syntaxes")));
        assert!(!safe_plugin_relative_path(Path::new("/syntaxes")));
        assert!(!safe_plugin_relative_path(Path::new("C:/syntaxes")));
        assert!(!safe_plugin_relative_path(Path::new("C:\\syntaxes")));
        assert!(!safe_plugin_relative_path(Path::new("C:syntaxes")));
        assert!(!safe_plugin_relative_path(Path::new(
            "\\\\server\\syntaxes"
        )));
    }
}
