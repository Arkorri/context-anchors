//! The bundled tree-sitter grammars and the queries that find comments and declarations in
//! each. Adding a language is one entry in [`LANGUAGE_TABLE`] plus its grammar crate.

use std::collections::HashMap;

use tree_sitter::{Language, Parser, Query, QueryError};

struct LanguageEntry {
    name: &'static str,
    extensions: &'static [&'static str],
    language: fn() -> Language,
    comment_query: &'static str,
    /// The grammar's own `tags.scm`.
    tags_query: &'static str,
    /// Declaration kinds `tags.scm` does not cover. Every pattern must tag a `@definition.*`
    /// capture alongside `@name`, or it is ignored.
    supplementary_declarations: &'static str,
}

const LANGUAGE_TABLE: &[LanguageEntry] = &[
    LanguageEntry {
        name: "rust",
        extensions: &["rs"],
        language: || tree_sitter_rust::LANGUAGE.into(),
        comment_query: "[(line_comment) (block_comment)] @comment",
        tags_query: tree_sitter_rust::TAGS_QUERY,
        supplementary_declarations: r#"
            (const_item name: (identifier) @name) @definition.constant
            (static_item name: (identifier) @name) @definition.constant
            (function_signature_item name: (identifier) @name) @definition.method
        "#,
    },
    LanguageEntry {
        name: "typescript",
        extensions: &["ts", "mts", "cts"],
        language: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        comment_query: "[(comment) (html_comment)] @comment",
        tags_query: tree_sitter_typescript::TAGS_QUERY,
        supplementary_declarations: TYPESCRIPT_SUPPLEMENTARY,
    },
    LanguageEntry {
        name: "tsx",
        extensions: &["tsx"],
        language: || tree_sitter_typescript::LANGUAGE_TSX.into(),
        comment_query: "[(comment) (html_comment)] @comment",
        tags_query: tree_sitter_typescript::TAGS_QUERY,
        supplementary_declarations: TYPESCRIPT_SUPPLEMENTARY,
    },
    LanguageEntry {
        name: "javascript",
        extensions: &["js", "mjs", "cjs", "jsx"],
        language: || tree_sitter_javascript::LANGUAGE.into(),
        comment_query: "[(comment) (html_comment)] @comment",
        tags_query: tree_sitter_javascript::TAGS_QUERY,
        supplementary_declarations: r#"
            (lexical_declaration (variable_declarator name: (identifier) @name)) @definition.constant
            (variable_declaration (variable_declarator name: (identifier) @name)) @definition.constant
            (field_definition property: (property_identifier) @name) @definition.field
        "#,
    },
    LanguageEntry {
        name: "python",
        extensions: &["py", "pyi"],
        language: || tree_sitter_python::LANGUAGE.into(),
        comment_query: "(comment) @comment",
        tags_query: tree_sitter_python::TAGS_QUERY,
        supplementary_declarations: "",
    },
    LanguageEntry {
        name: "go",
        extensions: &["go"],
        language: || tree_sitter_go::LANGUAGE.into(),
        comment_query: "(comment) @comment",
        tags_query: tree_sitter_go::TAGS_QUERY,
        supplementary_declarations: r#"
            (const_spec name: (identifier) @name) @definition.constant
            (var_spec name: (identifier) @name) @definition.variable
            (method_elem name: (field_identifier) @name) @definition.method
        "#,
    },
];

/// The TypeScript `tags.scm` covers only signatures, abstract classes, modules, and
/// interfaces; everyday declarations come from here.
const TYPESCRIPT_SUPPLEMENTARY: &str = r#"
    (function_declaration name: (identifier) @name) @definition.function
    (generator_function_declaration name: (identifier) @name) @definition.function
    (class_declaration name: (type_identifier) @name) @definition.class
    (method_definition name: (property_identifier) @name) @definition.method
    (public_field_definition name: (property_identifier) @name) @definition.field
    (type_alias_declaration name: (type_identifier) @name) @definition.type
    (enum_declaration name: (identifier) @name) @definition.enum
    (internal_module name: (identifier) @name) @definition.module
    (lexical_declaration (variable_declarator name: (identifier) @name)) @definition.constant
    (variable_declaration (variable_declarator name: (identifier) @name)) @definition.constant
"#;

/// One bundled language with its compiled queries.
pub struct LanguageSpec {
    name: &'static str,
    extensions: &'static [&'static str],
    language: Language,
    comment_query: Query,
    declaration_query: Query,
    name_capture_index: u32,
    definition_capture_indices: Vec<u32>,
}

impl LanguageSpec {
    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn extensions(&self) -> &'static [&'static str] {
        self.extensions
    }

    pub fn language(&self) -> &Language {
        &self.language
    }

    pub(crate) fn comment_query(&self) -> &Query {
        &self.comment_query
    }

    pub(crate) fn declaration_query(&self) -> &Query {
        &self.declaration_query
    }

    pub(crate) fn name_capture_index(&self) -> u32 {
        self.name_capture_index
    }

    pub(crate) fn is_definition_capture(&self, index: u32) -> bool {
        self.definition_capture_indices.contains(&index)
    }
}

impl std::fmt::Debug for LanguageSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LanguageSpec")
            .field("name", &self.name)
            .field("extensions", &self.extensions)
            .finish_non_exhaustive()
    }
}

impl PartialEq for LanguageSpec {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for LanguageSpec {}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("the `{language}` grammar rejected its {which} query: {source}")]
    Query {
        language: &'static str,
        which: &'static str,
        #[source]
        source: QueryError,
    },
    #[error("the `{language}` declaration query has no `@name` capture")]
    MissingNameCapture { language: &'static str },
    #[error("the `{language}` declaration query has no `@definition.*` capture")]
    NoDefinitionCaptures { language: &'static str },
    #[error("the `{language}` grammar is incompatible with this tree-sitter runtime")]
    IncompatibleLanguage { language: &'static str },
    #[error("extension `.{extension}` is claimed by both `{first}` and `{second}`")]
    DuplicateExtension {
        extension: &'static str,
        first: &'static str,
        second: &'static str,
    },
}

/// Every bundled language, keyed by file extension. Built once per run; queries are compiled
/// here so a grammar/query mismatch is a startup error, not a per-file surprise.
pub struct LanguageRegistry {
    specs: Vec<LanguageSpec>,
    by_extension: HashMap<&'static str, usize>,
}

impl LanguageRegistry {
    pub fn new() -> Result<Self, RegistryError> {
        let mut probe = Parser::new();
        let mut specs = Vec::with_capacity(LANGUAGE_TABLE.len());
        let mut by_extension = HashMap::new();
        for entry in LANGUAGE_TABLE {
            let spec = compile(entry, &mut probe)?;
            for extension in entry.extensions {
                if let Some(previous) = by_extension.insert(*extension, specs.len()) {
                    let first: &LanguageSpec = &specs[previous];
                    return Err(RegistryError::DuplicateExtension {
                        extension,
                        first: first.name,
                        second: entry.name,
                    });
                }
            }
            specs.push(spec);
        }
        Ok(Self {
            specs,
            by_extension,
        })
    }

    /// `extension` is lowercase without the dot.
    pub fn for_extension(&self, extension: &str) -> Option<&LanguageSpec> {
        self.by_extension
            .get(extension)
            .map(|index| &self.specs[*index])
    }

    pub fn languages(&self) -> impl Iterator<Item = &LanguageSpec> {
        self.specs.iter()
    }
}

fn compile(entry: &LanguageEntry, probe: &mut Parser) -> Result<LanguageSpec, RegistryError> {
    let language = (entry.language)();
    probe
        .set_language(&language)
        .map_err(|_| RegistryError::IncompatibleLanguage {
            language: entry.name,
        })?;

    let comment_query =
        Query::new(&language, entry.comment_query).map_err(|source| RegistryError::Query {
            language: entry.name,
            which: "comment",
            source,
        })?;

    let declaration_source = format!("{}\n{}", entry.tags_query, entry.supplementary_declarations);
    let declaration_query =
        Query::new(&language, &declaration_source).map_err(|source| RegistryError::Query {
            language: entry.name,
            which: "declaration",
            source,
        })?;

    let name_capture_index = declaration_query.capture_index_for_name("name").ok_or(
        RegistryError::MissingNameCapture {
            language: entry.name,
        },
    )?;
    let definition_capture_indices: Vec<u32> = declaration_query
        .capture_names()
        .iter()
        .enumerate()
        .filter(|(_, capture)| capture.starts_with("definition."))
        .filter_map(|(index, _)| u32::try_from(index).ok())
        .collect();
    if definition_capture_indices.is_empty() {
        return Err(RegistryError::NoDefinitionCaptures {
            language: entry.name,
        });
    }

    Ok(LanguageSpec {
        name: entry.name,
        extensions: entry.extensions,
        language,
        comment_query,
        declaration_query,
        name_capture_index,
        definition_capture_indices,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_grammar_and_query_compiles() {
        let registry = LanguageRegistry::new().unwrap();
        let names: Vec<&str> = registry.languages().map(LanguageSpec::name).collect();
        assert_eq!(
            names,
            vec!["rust", "typescript", "tsx", "javascript", "python", "go"]
        );
    }

    #[test]
    fn extensions_map_to_their_language() {
        let registry = LanguageRegistry::new().unwrap();
        for (extension, language) in [
            ("rs", "rust"),
            ("ts", "typescript"),
            ("mts", "typescript"),
            ("tsx", "tsx"),
            ("js", "javascript"),
            ("jsx", "javascript"),
            ("py", "python"),
            ("go", "go"),
        ] {
            assert_eq!(registry.for_extension(extension).unwrap().name(), language);
        }
        assert!(registry.for_extension("ex").is_none());
        assert!(
            registry.for_extension("RS").is_none(),
            "lookups are lowercase"
        );
    }
}
