//! The outermost definitions of one file, as tree-sitter parses them.
//!
//! A change segment splits at the boundaries of these definitions, so each
//! function, struct or class inside it gets its own viewed toggle. A parse
//! depends only on the blob's text, so the split does too.

use git2::Oid;
use std::collections::HashMap;
use std::sync::Arc;
use tree_sitter::{Language, Node, Parser};

/// One outermost definition: a function, struct, impl, class and so on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    /// Node kind and identifier, for example `function_item:parse`. A removed
    /// and an added definition with the same name are the same definition.
    pub name: String,
    /// First and last line, 1-based and inclusive.
    pub start: u32,
    pub end: u32,
}

/// The definitions of one blob, and the lines the parser could not read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outline {
    pub definitions: Vec<Definition>,
    /// Line ranges (1-based, inclusive) of ERROR and MISSING nodes. A segment
    /// that overlaps one is not split: the boundaries there are guesses.
    pub errors: Vec<(u32, u32)>,
}

/// The outlines of a file's two blobs. A side is `None` when the file is
/// absent there, is binary, or has no grammar.
#[derive(Debug, Clone, Default)]
pub struct FileOutlines {
    pub old: Option<Arc<Outline>>,
    pub new: Option<Arc<Outline>>,
}

/// Outlines already parsed, keyed on blob id and extension.
///
/// The segments of every file are worked out on every tree rebuild, and a
/// reload re-reads the whole diff. A blob's outline depends only on its content,
/// so it is parsed once for the life of the app.
#[derive(Default)]
pub struct OutlineCache {
    outlines: HashMap<(Oid, String), Option<Arc<Outline>>>,
}

impl OutlineCache {
    /// The outline of blob `oid`, parsed from `load()` on first sight.
    pub fn get(
        &mut self,
        oid: Oid,
        ext: &str,
        load: impl FnOnce() -> Option<Vec<u8>>,
    ) -> Option<Arc<Outline>> {
        self.outlines
            .entry((oid, ext.to_string()))
            .or_insert_with(|| {
                // Without a grammar there is nothing to parse, so skip the read.
                grammar(ext)?;
                load().and_then(|source| outline(&source, ext)).map(Arc::new)
            })
            .clone()
    }
}

/// The outline of `source` in the language of extension `ext`, or `None` for
/// a language without a grammar here.
pub fn outline(source: &[u8], ext: &str) -> Option<Outline> {
    let (language, kinds) = grammar(ext)?;
    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    let tree = parser.parse(source, None)?;

    let mut outline = Outline::default();
    collect(tree.root_node(), source, kinds, &mut outline);
    Some(outline)
}

const TYPESCRIPT: &[&str] = &[
    "function_declaration",
    "generator_function_declaration",
    "class_declaration",
    "abstract_class_declaration",
    "interface_declaration",
    "enum_declaration",
];

/// One grammar: the extensions it reads, its language, and the node kinds
/// that count as a definition in it.
type Grammar = (
    &'static [&'static str],
    fn() -> Language,
    &'static [&'static str],
);

const GRAMMARS: &[Grammar] = &[
    (
        &["rs"],
        || tree_sitter_rust::LANGUAGE.into(),
        &[
            "function_item",
            "struct_item",
            "enum_item",
            "union_item",
            "impl_item",
            "trait_item",
            "macro_definition",
        ],
    ),
    (
        &["py", "pyi"],
        || tree_sitter_python::LANGUAGE.into(),
        &[
            "function_definition",
            "class_definition",
            "decorated_definition",
        ],
    ),
    (
        &["js", "mjs", "cjs", "jsx"],
        || tree_sitter_javascript::LANGUAGE.into(),
        &[
            "function_declaration",
            "generator_function_declaration",
            "class_declaration",
        ],
    ),
    (
        &["ts", "mts", "cts"],
        || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        TYPESCRIPT,
    ),
    (
        &["tsx"],
        || tree_sitter_typescript::LANGUAGE_TSX.into(),
        TYPESCRIPT,
    ),
    (
        &["go"],
        || tree_sitter_go::LANGUAGE.into(),
        &[
            "function_declaration",
            "method_declaration",
            "type_declaration",
        ],
    ),
    (
        &["c", "h"],
        || tree_sitter_c::LANGUAGE.into(),
        &["function_definition"],
    ),
    (
        &["cpp", "cc", "cxx", "hpp", "hh", "hxx"],
        || tree_sitter_cpp::LANGUAGE.into(),
        &["function_definition", "class_specifier"],
    ),
    (
        &["java"],
        || tree_sitter_java::LANGUAGE.into(),
        &[
            "class_declaration",
            "interface_declaration",
            "enum_declaration",
            "record_declaration",
        ],
    ),
    (
        &["rb"],
        || tree_sitter_ruby::LANGUAGE.into(),
        &["method", "singleton_method", "class"],
    ),
    (
        &["sh", "bash"],
        || tree_sitter_bash::LANGUAGE.into(),
        &["function_definition"],
    ),
];

/// The grammar for extension `ext`, and the node kinds that count as a
/// definition in it.
fn grammar(ext: &str) -> Option<(Language, &'static [&'static str])> {
    GRAMMARS
        .iter()
        .find(|(exts, _, _)| exts.contains(&ext))
        .map(|&(_, language, kinds)| (language(), kinds))
}

/// Record the outermost definitions under `node`, and every ERROR or MISSING
/// node. A definition's children are not searched for definitions: a method
/// stays part of its impl.
fn collect(node: Node, source: &[u8], kinds: &[&str], outline: &mut Outline) {
    if kinds.contains(&node.kind()) {
        let (start, end) = lines(node);
        let name = format!("{}:{}", node.kind(), name(node, source));
        outline.definitions.push(Definition { name, start, end });
        collect_errors(node, outline);
        return;
    }
    if node.is_error() || node.is_missing() {
        outline.errors.push(lines(node));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(child, source, kinds, outline);
    }
}

fn collect_errors(node: Node, outline: &mut Outline) {
    if !node.has_error() {
        return;
    }
    if node.is_error() || node.is_missing() {
        outline.errors.push(lines(node));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_errors(child, outline);
    }
}

/// First and last line of `node`, 1-based and inclusive.
fn lines(node: Node) -> (u32, u32) {
    let (start, end) = (node.start_position(), node.end_position());
    // A node that ends at column 0 ends with the newline of the line before.
    let end_row = if end.column == 0 && end.row > start.row { end.row - 1 } else { end.row };
    (start.row as u32 + 1, end_row as u32 + 1)
}

/// The identifier of a definition: its `name` field, the name of the
/// definition it decorates, or else its first line.
fn name(node: Node, source: &[u8]) -> String {
    if let Some(name) = node.child_by_field_name("name") {
        return name.utf8_text(source).unwrap_or_default().to_string();
    }
    if let Some(inner) = node.child_by_field_name("definition") {
        return name(inner, source);
    }
    let text = node.utf8_text(source).unwrap_or_default();
    text.lines().next().unwrap_or_default().trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rust_file_lists_its_top_level_functions() {
        let source = b"use x;\n\nfn a() {\n    1\n}\n\nfn b() {}\n";

        let outline = outline(source, "rs").unwrap();

        assert_eq!(
            outline.definitions,
            vec![
                Definition { name: "function_item:a".into(), start: 3, end: 5 },
                Definition { name: "function_item:b".into(), start: 7, end: 7 },
            ]
        );
        assert!(outline.errors.is_empty());
    }

    #[test]
    fn a_syntax_error_is_reported_on_its_lines() {
        let source = b"fn a() {}\n\nfn b( {\n}\n";

        let outline = outline(source, "rs").unwrap();

        assert!(outline.errors.iter().any(|&(start, end)| start <= 3 && end >= 3));
    }

    #[test]
    fn a_method_stays_part_of_its_impl() {
        let source = b"impl A {\n    fn f() {}\n    fn g() {}\n}\n";

        let outline = outline(source, "rs").unwrap();

        assert_eq!(outline.definitions.len(), 1);
        assert_eq!((outline.definitions[0].start, outline.definitions[0].end), (1, 4));
    }

    #[test]
    fn an_unknown_extension_has_no_outline() {
        assert_eq!(outline(b"a b c", "md"), None);
    }

    #[test]
    fn every_definition_kind_exists_in_its_grammar() {
        let exts = ["rs", "py", "js", "ts", "tsx", "go", "c", "cpp", "java", "rb", "sh"];
        for ext in exts {
            let (language, kinds) = grammar(ext).unwrap_or_else(|| panic!("no grammar for {ext}"));
            for kind in kinds {
                assert_ne!(language.id_for_node_kind(kind, true), 0, "{ext}: no node kind {kind}");
            }
        }
    }

    #[test]
    fn a_decorated_python_function_is_named_after_the_function() {
        let source = b"@cache\ndef f():\n    pass\n\nclass C:\n    pass\n";

        let outline = outline(source, "py").unwrap();

        let names: Vec<&str> = outline.definitions.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["decorated_definition:f", "class_definition:C"]);
        assert_eq!((outline.definitions[0].start, outline.definitions[0].end), (1, 3));
    }

    #[test]
    fn a_blob_is_parsed_once() {
        let mut cache = OutlineCache::default();
        let oid = git2::Oid::from_str("1234").unwrap();

        let first = cache.get(oid, "rs", || Some(b"fn a() {}\n".to_vec()));
        let second = cache.get(oid, "rs", || panic!("read the blob again"));

        assert_eq!(first.unwrap().definitions.len(), 1);
        assert!(second.is_some());
    }

    #[test]
    fn a_go_file_lists_its_functions_and_types() {
        let source = b"package p\n\nfunc a() {\n}\n\ntype T struct{}\n";

        let outline = outline(source, "go").unwrap();

        let found: Vec<(&str, u32, u32)> =
            outline.definitions.iter().map(|d| (d.name.as_str(), d.start, d.end)).collect();
        assert_eq!(
            found,
            [("function_declaration:a", 3, 4), ("type_declaration:type T struct{}", 6, 6)]
        );
    }
}
