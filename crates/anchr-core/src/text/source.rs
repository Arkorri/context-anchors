use std::collections::HashMap;

use tree_sitter::{QueryCursor, StreamingIterator, Tree};

use super::{AnalyzeError, LanguageSpec, RegionKind, TextRegion, TextRegions};
use crate::marker::SymbolName;
use crate::span::ByteSpan;

pub const MAX_DECLARATIONS_PER_FILE: usize = 100_000;

/// The declarations found in one source file, by name, with the span of each name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SymbolTable {
    declarations: HashMap<String, Vec<ByteSpan>>,
    /// The tree contained ERROR nodes, so a declaration may have been invisible to the query.
    pub has_parse_errors: bool,
}

impl SymbolTable {
    pub fn contains(&self, name: &SymbolName) -> bool {
        self.declarations.contains_key(name.as_str())
    }

    pub fn spans(&self, name: &SymbolName) -> &[ByteSpan] {
        self.declarations
            .get(name.as_str())
            .map_or(&[], Vec::as_slice)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.declarations.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.declarations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.declarations.is_empty()
    }
}

pub(crate) fn comment_regions(spec: &LanguageSpec, tree: &Tree, source: &str) -> TextRegions {
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(spec.comment_query(), tree.root_node(), source.as_bytes());
    let mut regions = Vec::new();
    while let Some(found) = matches.next() {
        for capture in found.captures() {
            regions.push(TextRegion {
                span: ByteSpan::from(capture.node.byte_range()),
                kind: RegionKind::Comment,
            });
        }
    }
    TextRegions::new(regions)
}

/// Runs the declaration query and keeps the `@name` of every match that also carries a
/// `@definition.*` capture; `@reference.*` matches from `tags.scm` are dropped here.
pub(crate) fn symbol_table(
    spec: &LanguageSpec,
    tree: &Tree,
    source: &str,
) -> Result<SymbolTable, AnalyzeError> {
    let mut table = SymbolTable {
        declarations: HashMap::new(),
        has_parse_errors: tree.root_node().has_error(),
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(
        spec.declaration_query(),
        tree.root_node(),
        source.as_bytes(),
    );
    let mut recorded = 0usize;
    while let Some(found) = matches.next() {
        let captures = found.captures();
        if !captures
            .iter()
            .any(|capture| spec.is_definition_capture(capture.index))
        {
            continue;
        }
        for capture in captures
            .iter()
            .filter(|c| c.index == spec.name_capture_index())
        {
            let range = capture.node.byte_range();
            let Some(name) = source.get(range.clone()) else {
                continue;
            };
            recorded += 1;
            if recorded > MAX_DECLARATIONS_PER_FILE {
                return Err(AnalyzeError::SymbolTableTruncated);
            }
            table
                .declarations
                .entry(name.to_owned())
                .or_default()
                .push(ByteSpan::from(range));
        }
    }
    Ok(table)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::marker::lex;
    use crate::text::{FileAnalyzer, LanguageRegistry};

    fn analyzer(registry: &LanguageRegistry) -> FileAnalyzer<'_> {
        FileAnalyzer::new(registry, Duration::from_secs(10))
    }

    fn comment_marker_bodies<'s>(
        registry: &LanguageRegistry,
        extension: &str,
        source: &'s str,
    ) -> Vec<&'s str> {
        let spec = registry.for_extension(extension).unwrap();
        let regions = analyzer(registry)
            .text_regions(crate::text::Container::Source(spec), source)
            .unwrap();
        let lexed = lex(source, &regions).unwrap();
        assert!(lexed.malformed.is_empty(), "{:?}", lexed.malformed);
        lexed
            .markers
            .iter()
            .map(|marker| &source[marker.body_span.start..marker.body_span.end])
            .collect()
    }

    fn symbols(registry: &LanguageRegistry, extension: &str, source: &str) -> SymbolTable {
        let spec = registry.for_extension(extension).unwrap();
        analyzer(registry).symbols(spec, source).unwrap()
    }

    fn assert_declares(table: &SymbolTable, names: &[&str]) {
        let mut found: Vec<&str> = table.names().collect();
        found.sort_unstable();
        let mut missing: Vec<&&str> = names.iter().filter(|n| !found.contains(n)).collect();
        missing.sort_unstable();
        assert!(missing.is_empty(), "missing {missing:?}; found {found:?}");
        assert!(!table.has_parse_errors, "fixture must parse cleanly");
    }

    #[test]
    fn rust_comments_are_lexed_and_code_is_not() {
        let registry = LanguageRegistry::new().unwrap();
        let source = r#"
            //! Crate docs @ref[#crate]
            /// Doc comment @ref[a.rs#f]
            fn f() { let s = "@ref[in-string.md]"; /* block @anchor[b] /* nested @anchor[c] */ */ }
        "#;
        assert_eq!(
            comment_marker_bodies(&registry, "rs", source),
            vec!["#crate", "a.rs#f", "b", "c"]
        );
    }

    #[test]
    fn rust_declarations_of_every_kind_are_found() {
        let registry = LanguageRegistry::new().unwrap();
        let table = symbols(
            &registry,
            "rs",
            r#"
            pub struct Config;
            enum Mode { A }
            union Bits { a: u8 }
            type Alias = u8;
            trait Checks { fn required(&self); }
            impl Config { pub fn method(&self) {} }
            fn function() {}
            mod inner {}
            macro_rules! mac { () => {} }
            const LIMIT: usize = 1;
            static NAME: &str = "x";
            "#,
        );
        assert_declares(
            &table,
            &[
                "Config", "Mode", "Bits", "Alias", "Checks", "required", "method", "function",
                "inner", "mac", "LIMIT", "NAME",
            ],
        );
        assert_eq!(table.spans(&SymbolName::parse("Config").unwrap()).len(), 1);
    }

    #[test]
    fn typescript_declarations_of_every_kind_are_found() {
        let registry = LanguageRegistry::new().unwrap();
        let table = symbols(
            &registry,
            "ts",
            r#"
            // @ref[#ts]
            export function plain(): void {}
            function* gen() {}
            export const arrow = () => 1;
            const VALUE = 42;
            let mutable = "x";
            export class Widget { field = 1; method(): void {} }
            abstract class Base { abstract run(): void; }
            interface Shape { area(): number; }
            type Alias = string | number;
            enum Color { Red }
            namespace Util { export const x = 1; }
            declare function overload(a: string): void;
            "#,
        );
        assert_declares(
            &table,
            &[
                "plain", "gen", "arrow", "VALUE", "mutable", "Widget", "field", "method", "Base",
                "run", "Shape", "area", "Alias", "Color", "Util", "overload",
            ],
        );
        assert_eq!(
            comment_marker_bodies(&registry, "ts", "// @ref[#ts]\nconst s = '@ref[no]';"),
            vec!["#ts"]
        );
    }

    #[test]
    fn tsx_shares_the_typescript_queries() {
        let registry = LanguageRegistry::new().unwrap();
        let table = symbols(
            &registry,
            "tsx",
            "export const App = () => <div>{/* @ref[#jsx] */}</div>;\nfunction helper() {}\n",
        );
        assert_declares(&table, &["App", "helper"]);
        assert_eq!(
            comment_marker_bodies(
                &registry,
                "tsx",
                "const A = () => <div>{/* @ref[#jsx] */}</div>;"
            ),
            vec!["#jsx"]
        );
    }

    #[test]
    fn javascript_declarations_of_every_kind_are_found() {
        let registry = LanguageRegistry::new().unwrap();
        let table = symbols(
            &registry,
            "js",
            r#"
            /** @ref[#js] */
            function plain() {}
            const arrow = () => 1;
            const VALUE = 42;
            var legacy = 1;
            class Widget { field = 1; method() {} }
            module.exports.handler = function () {};
            "#,
        );
        assert_declares(
            &table,
            &[
                "plain", "arrow", "VALUE", "legacy", "Widget", "field", "method", "handler",
            ],
        );
        assert_eq!(
            comment_marker_bodies(&registry, "js", "/** @ref[#js] */ let s = `@ref[no]`;"),
            vec!["#js"]
        );
    }

    #[test]
    fn python_declarations_of_every_kind_are_found() {
        let registry = LanguageRegistry::new().unwrap();
        let source = r#"
# module comment @anchor[py]
LIMIT = 10

class Widget:
    """docstring is a string, not a comment: @ref[no.md]"""
    def method(self):  # trailing @ref[#py]
        pass

def function():
    pass
"#;
        let table = symbols(&registry, "py", source);
        assert_declares(&table, &["LIMIT", "Widget", "method", "function"]);
        assert_eq!(
            comment_marker_bodies(&registry, "py", source),
            vec!["py", "#py"]
        );
    }

    #[test]
    fn go_declarations_of_every_kind_are_found() {
        let registry = LanguageRegistry::new().unwrap();
        let source = r#"
package main

// @anchor[go]
const Limit = 10
var counter int

type Shape interface { Area() float64 }
type Point struct{ X, Y int }

func (p Point) Method() {}
func Function() {}
"#;
        let table = symbols(&registry, "go", source);
        assert_declares(
            &table,
            &[
                "Limit", "counter", "Shape", "Area", "Point", "Method", "Function",
            ],
        );
        assert_eq!(comment_marker_bodies(&registry, "go", source), vec!["go"]);
    }

    #[test]
    fn a_tree_with_errors_still_yields_comments_but_flags_the_symbol_table() {
        let registry = LanguageRegistry::new().unwrap();
        let source = "// @ref[#still-found]\nfn broken( { \nfn intact() {}\n";
        assert_eq!(
            comment_marker_bodies(&registry, "rs", source),
            vec!["#still-found"]
        );
        let table = symbols(&registry, "rs", source);
        assert!(table.has_parse_errors);
        assert!(table.contains(&SymbolName::parse("intact").unwrap()));
    }

    #[test]
    fn references_are_not_declarations() {
        let registry = LanguageRegistry::new().unwrap();
        let table = symbols(&registry, "rs", "fn caller() { callee(); Other::new(); }");
        assert!(table.contains(&SymbolName::parse("caller").unwrap()));
        assert!(!table.contains(&SymbolName::parse("callee").unwrap()));
        assert!(!table.contains(&SymbolName::parse("Other").unwrap()));
    }
}
