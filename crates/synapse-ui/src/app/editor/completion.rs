use std::ops::Range;

use super::code_block::{CodeLanguage, fenced_code_block_at};

const MAX_LOCAL_COMPLETIONS: usize = 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app) enum CompletionKind {
    Keyword,
    Snippet,
    Lsp,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app) struct CodeCompletionItem {
    pub(in crate::app) label: String,
    pub(in crate::app) detail: String,
    pub(in crate::app) insert_text: String,
    /// Character offset from the replacement start after applying the item.
    pub(in crate::app) cursor_offset: usize,
    pub(in crate::app) kind: CompletionKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app) struct CodeCompletionContext {
    pub(in crate::app) language: CodeLanguage,
    pub(in crate::app) code: String,
    pub(in crate::app) code_range: Range<usize>,
    pub(in crate::app) replacement_range: Range<usize>,
    pub(in crate::app) prefix: String,
    pub(in crate::app) cursor_in_code: usize,
    pub(in crate::app) line: usize,
    pub(in crate::app) utf16_column: usize,
}

pub(in crate::app) fn code_completion_context(
    source: &str,
    cursor: usize,
) -> Option<CodeCompletionContext> {
    let block = fenced_code_block_at(source, &(cursor..cursor))?;
    let code = slice_chars(source, block.content.clone());
    let cursor_in_code = cursor
        .saturating_sub(block.content.start)
        .min(code.chars().count());
    let prefix_start = completion_prefix_start(&code, cursor_in_code);
    let prefix = slice_chars(&code, prefix_start..cursor_in_code);
    let (line, utf16_column) = lsp_position(&code, cursor_in_code);
    Some(CodeCompletionContext {
        language: block.language,
        code,
        code_range: block.content.clone(),
        replacement_range: block.content.start + prefix_start..cursor,
        prefix,
        cursor_in_code,
        line,
        utf16_column,
    })
}

pub(in crate::app) fn local_code_completions_with_empty_prefix(
    context: &CodeCompletionContext,
    include_empty_prefix: bool,
) -> Vec<CodeCompletionItem> {
    if context.prefix.is_empty() && !include_empty_prefix {
        return Vec::new();
    }
    let query = context.prefix.to_ascii_lowercase();
    language_templates(context.language)
        .iter()
        .filter(|template| template.label.to_ascii_lowercase().starts_with(&query))
        .take(MAX_LOCAL_COMPLETIONS)
        .map(|template| CodeCompletionItem {
            label: template.label.to_owned(),
            detail: template.detail.to_owned(),
            insert_text: template.insert_text.to_owned(),
            cursor_offset: template.cursor_offset,
            kind: template.kind,
        })
        .collect()
}

pub(in crate::app) fn merge_code_completions(
    local: Vec<CodeCompletionItem>,
    remote: Vec<CodeCompletionItem>,
) -> Vec<CodeCompletionItem> {
    let mut merged = local;
    for item in remote {
        let duplicate = merged.iter().any(|existing| {
            existing.label.eq_ignore_ascii_case(&item.label)
                && existing.insert_text == item.insert_text
        });
        if !duplicate {
            merged.push(item);
        }
    }
    merged.truncate(MAX_LOCAL_COMPLETIONS);
    merged
}

pub(in crate::app) fn language_identifier(language: CodeLanguage) -> &'static str {
    match language {
        CodeLanguage::Rust => "rust",
        CodeLanguage::JavaScript => "javascript",
        CodeLanguage::TypeScript => "typescript",
        CodeLanguage::Python => "python",
        CodeLanguage::Go => "go",
        CodeLanguage::Java => "java",
        CodeLanguage::C => "c",
        CodeLanguage::Cpp => "cpp",
        CodeLanguage::CSharp => "csharp",
        CodeLanguage::Json => "json",
        CodeLanguage::Yaml => "yaml",
        CodeLanguage::Toml => "toml",
        CodeLanguage::Sql => "sql",
        CodeLanguage::Shell => "shellscript",
        CodeLanguage::Html => "html",
        CodeLanguage::Css => "css",
        CodeLanguage::Ruby => "ruby",
        CodeLanguage::Lua => "lua",
        CodeLanguage::Make => "makefile",
        CodeLanguage::Other => "plaintext",
    }
}

pub(in crate::app) fn language_file_extension(language: CodeLanguage) -> &'static str {
    match language {
        CodeLanguage::Rust => "rs",
        CodeLanguage::JavaScript => "js",
        CodeLanguage::TypeScript => "ts",
        CodeLanguage::Python => "py",
        CodeLanguage::Go => "go",
        CodeLanguage::Java => "java",
        CodeLanguage::C => "c",
        CodeLanguage::Cpp => "cpp",
        CodeLanguage::CSharp => "cs",
        CodeLanguage::Json => "json",
        CodeLanguage::Yaml => "yaml",
        CodeLanguage::Toml => "toml",
        CodeLanguage::Sql => "sql",
        CodeLanguage::Shell => "sh",
        CodeLanguage::Html => "html",
        CodeLanguage::Css => "css",
        CodeLanguage::Ruby => "rb",
        CodeLanguage::Lua => "lua",
        CodeLanguage::Make => "mk",
        CodeLanguage::Other => "txt",
    }
}

pub(super) fn lsp_position(code: &str, cursor: usize) -> (usize, usize) {
    let prefix = code.chars().take(cursor).collect::<String>();
    let line = prefix
        .chars()
        .filter(|character| *character == '\n')
        .count();
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix.as_str(), |(_, line)| line);
    (line, column.encode_utf16().count())
}

fn completion_prefix_start(code: &str, cursor: usize) -> usize {
    let characters = code.chars().collect::<Vec<_>>();
    let mut start = cursor.min(characters.len());
    while start > 0 && is_completion_identifier_character(characters[start - 1]) {
        start -= 1;
    }
    start
}

fn is_completion_identifier_character(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

fn slice_chars(source: &str, range: Range<usize>) -> String {
    source.chars().skip(range.start).take(range.len()).collect()
}

#[derive(Clone, Copy)]
struct CompletionTemplate {
    label: &'static str,
    detail: &'static str,
    insert_text: &'static str,
    cursor_offset: usize,
    kind: CompletionKind,
}

const fn keyword(label: &'static str) -> CompletionTemplate {
    CompletionTemplate {
        label,
        detail: "keyword",
        insert_text: label,
        cursor_offset: label.len(),
        kind: CompletionKind::Keyword,
    }
}

const fn snippet(
    label: &'static str,
    detail: &'static str,
    insert_text: &'static str,
    cursor_offset: usize,
) -> CompletionTemplate {
    CompletionTemplate {
        label,
        detail,
        insert_text,
        cursor_offset,
        kind: CompletionKind::Snippet,
    }
}

fn language_templates(language: CodeLanguage) -> &'static [CompletionTemplate] {
    const RUST: &[CompletionTemplate] = &[
        snippet("fn", "function", "fn name() {\n    \n}", 3),
        snippet("impl", "implementation block", "impl Type {\n    \n}", 5),
        snippet("match", "match expression", "match value {\n    \n}", 6),
        snippet("for", "for loop", "for item in items {\n    \n}", 4),
        keyword("async"),
        keyword("await"),
        keyword("const"),
        keyword("else"),
        keyword("enum"),
        keyword("if"),
        keyword("let"),
        keyword("loop"),
        keyword("mod"),
        keyword("mut"),
        keyword("pub"),
        keyword("return"),
        keyword("struct"),
        keyword("trait"),
        keyword("use"),
    ];
    const JS: &[CompletionTemplate] = &[
        snippet("function", "function", "function name() {\n  \n}", 9),
        snippet("for", "for loop", "for (const item of items) {\n  \n}", 11),
        snippet("if", "if statement", "if (condition) {\n  \n}", 4),
        snippet("try", "try/catch", "try {\n  \n} catch (error) {\n  \n}", 6),
        keyword("async"),
        keyword("await"),
        keyword("class"),
        keyword("const"),
        keyword("else"),
        keyword("export"),
        keyword("import"),
        keyword("let"),
        keyword("new"),
        keyword("return"),
        keyword("throw"),
    ];
    const TS: &[CompletionTemplate] = &[
        snippet("interface", "interface", "interface Name {\n  \n}", 10),
        snippet("type", "type alias", "type Name = {\n  \n};", 5),
        snippet("function", "function", "function name(): void {\n  \n}", 9),
        snippet("for", "for loop", "for (const item of items) {\n  \n}", 11),
        keyword("async"),
        keyword("await"),
        keyword("const"),
        keyword("export"),
        keyword("import"),
        keyword("implements"),
        keyword("readonly"),
        keyword("return"),
    ];
    const PYTHON: &[CompletionTemplate] = &[
        snippet("def", "function", "def name():\n    ", 4),
        snippet("class", "class", "class Name:\n    ", 6),
        snippet("for", "for loop", "for item in items:\n    ", 4),
        snippet("if", "if statement", "if condition:\n    ", 3),
        keyword("async"),
        keyword("await"),
        keyword("elif"),
        keyword("else"),
        keyword("from"),
        keyword("import"),
        keyword("lambda"),
        keyword("return"),
        keyword("try"),
        keyword("with"),
    ];
    const GO: &[CompletionTemplate] = &[
        snippet("func", "function", "func name() {\n\t\n}", 5),
        snippet("for", "for loop", "for condition {\n\t\n}", 4),
        snippet("if", "if statement", "if condition {\n\t\n}", 3),
        keyword("const"),
        keyword("defer"),
        keyword("go"),
        keyword("import"),
        keyword("interface"),
        keyword("package"),
        keyword("range"),
        keyword("return"),
        keyword("struct"),
        keyword("var"),
    ];
    const C_FAMILY: &[CompletionTemplate] = &[
        snippet("for", "for loop", "for (;;) {\n    \n}", 5),
        snippet("if", "if statement", "if (condition) {\n    \n}", 4),
        snippet("switch", "switch statement", "switch (value) {\n    \n}", 8),
        keyword("break"),
        keyword("class"),
        keyword("const"),
        keyword("else"),
        keyword("enum"),
        keyword("private"),
        keyword("public"),
        keyword("return"),
        keyword("static"),
        keyword("struct"),
        keyword("void"),
    ];
    const DATA: &[CompletionTemplate] = &[keyword("false"), keyword("null"), keyword("true")];
    const SQL: &[CompletionTemplate] = &[
        keyword("ALTER"),
        keyword("CREATE"),
        keyword("DELETE"),
        keyword("FROM"),
        keyword("GROUP BY"),
        keyword("INSERT INTO"),
        keyword("JOIN"),
        keyword("LIMIT"),
        keyword("ORDER BY"),
        keyword("SELECT"),
        keyword("UPDATE"),
        keyword("WHERE"),
    ];
    const SHELL: &[CompletionTemplate] = &[
        snippet("if", "if statement", "if condition; then\n  \nfi", 3),
        snippet("for", "for loop", "for item in items; do\n  \ndone", 4),
        keyword("case"),
        keyword("done"),
        keyword("echo"),
        keyword("export"),
        keyword("fi"),
        keyword("function"),
        keyword("then"),
    ];
    const WEB: &[CompletionTemplate] = &[
        keyword("class"),
        keyword("div"),
        keyword("display"),
        keyword("flex"),
        keyword("height"),
        keyword("margin"),
        keyword("padding"),
        keyword("width"),
    ];
    const GENERIC: &[CompletionTemplate] = &[
        keyword("else"),
        keyword("false"),
        keyword("if"),
        keyword("return"),
        keyword("true"),
    ];

    match language {
        CodeLanguage::Rust => RUST,
        CodeLanguage::JavaScript => JS,
        CodeLanguage::TypeScript => TS,
        CodeLanguage::Python => PYTHON,
        CodeLanguage::Go => GO,
        CodeLanguage::Java | CodeLanguage::C | CodeLanguage::Cpp | CodeLanguage::CSharp => C_FAMILY,
        CodeLanguage::Json | CodeLanguage::Yaml | CodeLanguage::Toml => DATA,
        CodeLanguage::Sql => SQL,
        CodeLanguage::Shell => SHELL,
        CodeLanguage::Html | CodeLanguage::Css => WEB,
        _ => GENERIC,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CodeCompletionItem, CompletionKind, code_completion_context,
        local_code_completions_with_empty_prefix, merge_code_completions,
    };

    #[test]
    fn completion_context_uses_the_fenced_code_prefix_and_utf16_position() {
        let source = "```rust\nfn main() {\n    ret\n}\n```";
        let cursor = source.find("ret").expect("prefix") + 3;
        let cursor = source[..cursor].chars().count();
        let context = code_completion_context(source, cursor).expect("code context");

        assert_eq!(context.prefix, "ret");
        assert_eq!(context.replacement_range, 24..27);
        assert_eq!(context.line, 1);
        assert_eq!(context.utf16_column, 7);
    }

    #[test]
    fn local_completion_returns_language_specific_snippets() {
        let source = "```python\ndef\n```";
        let context = code_completion_context(source, 13).expect("code context");
        let completions = local_code_completions_with_empty_prefix(&context, false);

        assert_eq!(completions[0].label, "def");
        assert_eq!(completions[0].kind, CompletionKind::Snippet);
        assert_eq!(completions[0].insert_text, "def name():\n    ");
    }

    #[test]
    fn lsp_items_extend_without_duplicating_local_candidates() {
        let local = vec![CodeCompletionItem {
            label: "return".to_owned(),
            detail: "keyword".to_owned(),
            insert_text: "return".to_owned(),
            cursor_offset: 6,
            kind: CompletionKind::Keyword,
        }];
        let remote = vec![
            CodeCompletionItem {
                label: "return".to_owned(),
                detail: "keyword".to_owned(),
                insert_text: "return".to_owned(),
                cursor_offset: 6,
                kind: CompletionKind::Lsp,
            },
            CodeCompletionItem {
                label: "result".to_owned(),
                detail: "local variable".to_owned(),
                insert_text: "result".to_owned(),
                cursor_offset: 6,
                kind: CompletionKind::Lsp,
            },
        ];

        let merged = merge_code_completions(local, remote);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[1].label, "result");
    }
}
