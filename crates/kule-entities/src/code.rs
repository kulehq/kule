//! Code, read with tree-sitter: each language names the syntax nodes that
//! define something, and how to find what they are called.

use std::ops::ControlFlow;
use std::time::Instant;

use tree_sitter::{Node, ParseOptions, ParseState, Parser};

use crate::{Entity, Kind, MAX_ENTITIES, PARSE_TIME, Skip, Spanned, own_bytes, tidy};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lang {
    Rust,
    CSharp,
    TypeScript,
    Tsx,
    JavaScript,
    Python,
    Go,
}

impl Lang {
    pub(crate) fn of(ext: &str) -> Option<Lang> {
        Some(match ext {
            "rs" => Lang::Rust,
            "cs" => Lang::CSharp,
            "ts" | "mts" | "cts" => Lang::TypeScript,
            "tsx" => Lang::Tsx,
            "js" | "mjs" | "cjs" | "jsx" => Lang::JavaScript,
            "py" | "pyi" => Lang::Python,
            "go" => Lang::Go,
            _ => return None,
        })
    }

    fn grammar(self) -> tree_sitter::Language {
        match self {
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
            Lang::Go => tree_sitter_go::LANGUAGE.into(),
        }
    }

    /// Between a name and the names inside it.
    fn separator(self) -> &'static str {
        match self {
            Lang::Rust => "::",
            _ => ".",
        }
    }

    fn rules(self) -> &'static [Rule] {
        match self {
            Lang::Rust => RUST,
            Lang::CSharp => CSHARP,
            Lang::TypeScript | Lang::Tsx | Lang::JavaScript => SCRIPT,
            Lang::Python => PYTHON,
            Lang::Go => GO,
        }
    }

    /// Nodes just before a definition that belong to it: its comments and,
    /// in Rust, its attributes.
    fn leading(self) -> &'static [&'static str] {
        match self {
            Lang::Rust => &["line_comment", "block_comment", "attribute_item"],
            _ => &["comment"],
        }
    }
}

/// A syntax node that defines an entity.
struct Rule {
    node: &'static str,
    kind: Kind,
    name: Name,
}

/// How a definition's name is found.
#[derive(Clone, Copy)]
enum Name {
    /// The first of these fields there is.
    Field(&'static [&'static str]),
    /// Every child in this field, joined (`const a, b`).
    All(&'static str),
    /// The `name` of each `variable_declarator` in it, or in its
    /// `variable_declaration` (C#).
    Declarators,
    /// Rust's `impl Type` or `impl Trait for Type`.
    RustImpl,
    /// Go's `func (t *Type) Name`.
    GoMethod,
    /// A Python assignment to a plain name.
    PythonAssignment,
    Fixed(&'static str),
}

const NAME: Name = Name::Field(&["name"]);

const fn rule(node: &'static str, kind: Kind, name: Name) -> Rule {
    Rule { node, kind, name }
}

const RUST: &[Rule] = &[
    rule("function_item", Kind::Function, NAME),
    rule("function_signature_item", Kind::Function, NAME),
    rule("struct_item", Kind::Struct, NAME),
    rule("union_item", Kind::Struct, NAME),
    rule("enum_item", Kind::Enum, NAME),
    rule("enum_variant", Kind::Variant, NAME),
    rule("trait_item", Kind::Trait, NAME),
    rule("impl_item", Kind::Impl, Name::RustImpl),
    rule("mod_item", Kind::Module, NAME),
    rule("type_item", Kind::Type, NAME),
    rule("associated_type", Kind::Type, NAME),
    rule("const_item", Kind::Constant, NAME),
    rule("static_item", Kind::Constant, NAME),
    rule("macro_definition", Kind::Macro, NAME),
    rule("field_declaration", Kind::Field, NAME),
];

const CSHARP: &[Rule] = &[
    rule("namespace_declaration", Kind::Namespace, NAME),
    rule("class_declaration", Kind::Class, NAME),
    rule("struct_declaration", Kind::Struct, NAME),
    rule("record_declaration", Kind::Record, NAME),
    rule("record_struct_declaration", Kind::Record, NAME),
    rule("interface_declaration", Kind::Interface, NAME),
    rule("enum_declaration", Kind::Enum, NAME),
    rule("enum_member_declaration", Kind::Variant, NAME),
    rule("delegate_declaration", Kind::Type, NAME),
    rule("method_declaration", Kind::Method, NAME),
    rule("constructor_declaration", Kind::Constructor, NAME),
    rule("destructor_declaration", Kind::Method, Name::Fixed("~")),
    rule(
        "operator_declaration",
        Kind::Method,
        Name::Fixed("operator"),
    ),
    rule(
        "conversion_operator_declaration",
        Kind::Method,
        Name::Fixed("operator"),
    ),
    rule("property_declaration", Kind::Property, NAME),
    rule("indexer_declaration", Kind::Indexer, Name::Fixed("this[]")),
    rule("event_declaration", Kind::Event, NAME),
    rule("event_field_declaration", Kind::Event, Name::Declarators),
    rule("field_declaration", Kind::Field, Name::Declarators),
];

/// TypeScript, TSX and JavaScript.
const SCRIPT: &[Rule] = &[
    rule("function_declaration", Kind::Function, NAME),
    rule("generator_function_declaration", Kind::Function, NAME),
    rule("function_signature", Kind::Function, NAME),
    rule("class_declaration", Kind::Class, NAME),
    rule("abstract_class_declaration", Kind::Class, NAME),
    rule("interface_declaration", Kind::Interface, NAME),
    rule("type_alias_declaration", Kind::Type, NAME),
    rule("enum_declaration", Kind::Enum, NAME),
    rule("internal_module", Kind::Namespace, NAME),
    rule("module", Kind::Namespace, NAME),
    rule("method_definition", Kind::Method, NAME),
    rule("method_signature", Kind::Method, NAME),
    rule("abstract_method_signature", Kind::Method, NAME),
    rule("public_field_definition", Kind::Field, NAME),
    rule("field_definition", Kind::Field, Name::Field(&["property"])),
    rule("property_signature", Kind::Property, NAME),
    rule("lexical_declaration", Kind::Variable, Name::Declarators),
    rule("variable_declaration", Kind::Variable, Name::Declarators),
];

const PYTHON: &[Rule] = &[
    rule("function_definition", Kind::Function, NAME),
    rule("class_definition", Kind::Class, NAME),
    rule(
        "expression_statement",
        Kind::Variable,
        Name::PythonAssignment,
    ),
];

const GO: &[Rule] = &[
    rule("function_declaration", Kind::Function, NAME),
    rule("method_declaration", Kind::Method, Name::GoMethod),
    rule("method_elem", Kind::Method, NAME),
    // A struct or an interface when that is what it names; see `kind`.
    rule("type_spec", Kind::Type, NAME),
    rule("type_alias", Kind::Type, NAME),
    rule("const_spec", Kind::Constant, Name::All("name")),
    rule("var_spec", Kind::Variable, Name::All("name")),
    rule("field_declaration", Kind::Field, Name::All("name")),
];

/// Nodes around a definition that are part of it when it is the only thing
/// they hold, and the field that holds it (or `None`: the only named child).
const WRAPPERS: &[(&str, Option<&str>)] = &[
    ("export_statement", Some("declaration")),
    ("decorated_definition", Some("definition")),
    ("type_declaration", None),
    ("const_declaration", None),
    ("var_declaration", None),
];

/// Kinds that hold methods: a function inside one is a method.
const TYPES: &[Kind] = &[
    Kind::Class,
    Kind::Struct,
    Kind::Record,
    Kind::Enum,
    Kind::Trait,
    Kind::Interface,
    Kind::Impl,
];

pub(crate) fn entities(lang: Lang, src: &[u8]) -> Result<Vec<Entity>, Skip> {
    let mut parser = Parser::new();
    parser
        .set_language(&lang.grammar())
        .map_err(|_| Skip::Unparsed)?;
    let deadline = Instant::now() + PARSE_TIME;
    let mut progress = |_: &ParseState| {
        if Instant::now() > deadline {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let tree = parser
        .parse_with_options(
            &mut |at, _| &src[at.min(src.len())..],
            None,
            Some(ParseOptions::new().progress_callback(&mut progress)),
        )
        .ok_or(Skip::Unparsed)?;

    let breaks: Vec<usize> = src
        .iter()
        .enumerate()
        .filter(|&(_, &b)| b == b'\n')
        .map(|(i, _)| i)
        .collect();
    let mut spanned = vec![Spanned {
        kind: Kind::TopLevel,
        name: String::new(),
        parent: None,
        start: 0,
        end: src.len(),
        line: 1,
    }];
    // Depth first, without recursion: nesting is the file's to choose.
    // Each node comes with the entity it is in and whether that is (in) a
    // function, where variables are locals rather than entities.
    let mut stack: Vec<(Node, usize, bool)> = vec![(tree.root_node(), 0, false)];
    let mut cursor = tree.walk();
    while let Some((node, parent, in_function)) = stack.pop() {
        let mut inside = (parent, in_function);
        if let Some(rule) = lang.rules().iter().find(|r| r.node == node.kind()) {
            let kind = kind(lang, rule, node, &spanned[parent], in_function);
            let name = kind.and_then(|_| name(rule.name, node, src));
            if let (Some(kind), Some(name)) = (kind, name) {
                if spanned.len() > MAX_ENTITIES {
                    return Err(Skip::TooMany);
                }
                let outer = wrapped(node);
                let start = leading(lang, outer, src);
                let qualified = match &spanned[parent] {
                    p if p.kind == Kind::TopLevel => name,
                    p => format!("{}{}{name}", p.name, lang.separator()),
                };
                spanned.push(Spanned {
                    kind,
                    name: qualified,
                    parent: Some(parent),
                    start,
                    end: outer.end_byte(),
                    line: line_of(&breaks, start),
                });
                let function = matches!(kind, Kind::Function | Kind::Method | Kind::Constructor);
                inside = (spanned.len() - 1, in_function || function);
            }
        }
        let children: Vec<Node> = node.named_children(&mut cursor).collect();
        for child in children.into_iter().rev() {
            stack.push((child, inside.0, inside.1));
        }
    }
    Ok(own_bytes(src, spanned))
}

/// What the rule's node is here, or `None` when it is not an entity here.
fn kind(lang: Lang, rule: &Rule, node: Node, parent: &Spanned, in_function: bool) -> Option<Kind> {
    let kind = match rule.kind {
        Kind::Function if TYPES.contains(&parent.kind) => Kind::Method,
        Kind::Variable | Kind::Constant | Kind::Field if in_function => return None,
        Kind::Variable if lang == Lang::Python && parent.kind == Kind::Class => Kind::Field,
        Kind::Type if node.kind() == "type_spec" => {
            match node.child_by_field_name("type").map(|t| t.kind()) {
                Some("struct_type") => Kind::Struct,
                Some("interface_type") => Kind::Interface,
                _ => Kind::Type,
            }
        }
        kind => kind,
    };
    Some(kind)
}

fn name(how: Name, node: Node, src: &[u8]) -> Option<String> {
    let text = |n: Node| String::from_utf8_lossy(&src[n.byte_range()]).into_owned();
    let joined = |names: Vec<String>| (!names.is_empty()).then(|| tidy(&names.join(", ")));
    match how {
        Name::Field(fields) => fields
            .iter()
            .find_map(|f| node.child_by_field_name(f))
            .map(|n| tidy(&text(n))),
        Name::All(field) => {
            let mut cursor = node.walk();
            let names: Vec<String> = node
                .children_by_field_name(field, &mut cursor)
                .map(text)
                .collect();
            // An embedded Go field has a type and no name.
            joined(names).or_else(|| node.child_by_field_name("type").map(|t| tidy(&text(t))))
        }
        Name::Declarators => {
            let mut cursor = node.walk();
            let holder = node
                .named_children(&mut cursor)
                .find(|c| c.kind() == "variable_declaration")
                .unwrap_or(node);
            let mut cursor = holder.walk();
            let names: Vec<String> = holder
                .named_children(&mut cursor)
                .filter(|c| c.kind() == "variable_declarator")
                .filter_map(|c| c.child_by_field_name("name"))
                .map(text)
                .collect();
            joined(names)
        }
        Name::RustImpl => {
            let ty = text(node.child_by_field_name("type")?);
            Some(tidy(&match node.child_by_field_name("trait") {
                Some(t) => format!("<{ty} as {}>", text(t)),
                None => ty,
            }))
        }
        Name::GoMethod => {
            let name = text(node.child_by_field_name("name")?);
            let receiver = node
                .child_by_field_name("receiver")
                .and_then(|r| r.named_child(0))
                .and_then(|p| p.child_by_field_name("type"))
                .map(|t| text(t).trim_start_matches('*').to_owned());
            Some(tidy(&match receiver {
                Some(receiver) => format!("{receiver}.{name}"),
                None => name,
            }))
        }
        Name::PythonAssignment => {
            let assignment = node.named_child(0).filter(|a| a.kind() == "assignment")?;
            let left = assignment
                .child_by_field_name("left")
                .filter(|l| l.kind() == "identifier")?;
            Some(tidy(&text(left)))
        }
        Name::Fixed(name) => Some(name.to_owned()),
    }
}

/// The node itself, or the wrapper around it that holds nothing else
/// (`export function f`, a decorated Python function, Go's `type T struct`).
fn wrapped(node: Node) -> Node {
    let Some(parent) = node.parent() else {
        return node;
    };
    let only = match WRAPPERS.iter().find(|(kind, _)| *kind == parent.kind()) {
        Some((_, Some(field))) => parent.child_by_field_name(field) == Some(node),
        Some((_, None)) => {
            let mut cursor = parent.walk();
            parent
                .named_children(&mut cursor)
                .filter(|c| c.kind() != "comment")
                .count()
                == 1
        }
        None => false,
    };
    if only { parent } else { node }
}

/// Where the definition starts with the comments and attributes right
/// above it: each on lines of its own, with no blank line in between.
fn leading(lang: Lang, node: Node, src: &[u8]) -> usize {
    let mut start = node.start_byte();
    let mut prev = node.prev_named_sibling();
    while let Some(p) = prev {
        if !lang.leading().contains(&p.kind()) {
            break;
        }
        let gap = &src[p.end_byte().min(start)..start];
        let newlines = gap.iter().filter(|&&b| b == b'\n').count()
            + usize::from(src[..p.end_byte()].ends_with(b"\n"));
        let line_start = src[..p.start_byte()]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |i| i + 1);
        let own_line = src[line_start..p.start_byte()]
            .iter()
            .all(u8::is_ascii_whitespace);
        if !gap.iter().all(u8::is_ascii_whitespace) || newlines > 1 || !own_line {
            break;
        }
        start = p.start_byte();
        prev = p.prev_named_sibling();
    }
    start
}

/// The 1-based line of `at`, given where each line break is.
fn line_of(breaks: &[usize], at: usize) -> u32 {
    u32::try_from(breaks.partition_point(|&b| b < at) + 1).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use crate::tests::changes;
    use crate::{Edit, Kind};

    use Edit::*;

    fn c(edit: Edit, kind: Kind, name: &str) -> (Edit, Kind, String) {
        (edit, kind, name.to_owned())
    }

    #[test]
    fn rust_methods_fields_and_items() {
        let before = "use a;\n\
            pub struct Task {\n    id: String,\n    home: u8,\n}\n\
            impl Task {\n    pub fn commit(&self) -> u8 { 1 }\n    fn keep(&self) {}\n}\n\
            const MAX: usize = 1;\n";
        let after = "use a;\nuse b;\n\
            pub struct Task {\n    id: String,\n    trash: u8,\n}\n\
            impl Task {\n    pub fn commit(&self) -> u8 { 2 }\n    fn keep(&self) {}\n    fn trash_dir(&self) {}\n}\n";
        assert_eq!(
            changes("src/task.rs", before, after),
            vec![
                c(Modified, Kind::TopLevel, ""),
                c(Removed, Kind::Field, "Task::home"),
                c(Added, Kind::Field, "Task::trash"),
                c(Modified, Kind::Method, "Task::commit"),
                c(Removed, Kind::Constant, "MAX"),
                c(Added, Kind::Method, "Task::trash_dir"),
            ]
        );
    }

    #[test]
    fn a_change_inside_a_method_is_the_methods_alone() {
        let before = "impl A {\n    fn a(&self) { 1 }\n    fn b(&self) {}\n}\n";
        let after = "impl A {\n    fn a(&self) { 2 }\n    fn b(&self) {}\n}\n";
        assert_eq!(
            changes("a.rs", before, after),
            vec![c(Modified, Kind::Method, "A::a")]
        );
    }

    #[test]
    fn adding_something_changes_nothing_around_it() {
        let before = "use a;\n\nstruct S {\n    x: u8,\n}\n\nfn f() {}\n";
        let after = "use a;\nfn e() {}\n\nstruct S {\n    x: u8,\n    y: u8,\n}\n\nfn f() {}\n\nfn g() {}\n";
        assert_eq!(
            changes("a.rs", before, after),
            vec![
                c(Added, Kind::Function, "e"),
                c(Added, Kind::Field, "S::y"),
                c(Added, Kind::Function, "g"),
            ]
        );
    }

    #[test]
    fn indentation_inside_a_body_counts() {
        let before = "def f(x):\n    if x:\n        y()\n    z()\n";
        let after = "def f(x):\n    if x:\n        y()\n        z()\n";
        assert_eq!(
            changes("a.py", before, after),
            vec![c(Modified, Kind::Function, "f")]
        );
    }

    #[test]
    fn what_comes_with_a_new_type_is_not_listed_apart() {
        let after = "struct A { x: u8 }\nimpl A {\n    fn a(&self) {}\n}\n";
        assert_eq!(
            changes("a.rs", "", after),
            vec![c(Added, Kind::Struct, "A"), c(Added, Kind::Impl, "A")]
        );
        assert_eq!(
            changes("a.rs", after, ""),
            vec![c(Removed, Kind::Struct, "A"), c(Removed, Kind::Impl, "A")]
        );
    }

    #[test]
    fn comments_and_attributes_belong_to_what_follows() {
        let before = "fn a() {}\n\n/// Does b.\n#[inline]\nfn b() {}\n";
        let after = "fn a() {}\n\n/// Does b well.\n#[inline]\nfn b() {}\n";
        assert_eq!(
            changes("a.rs", before, after),
            vec![c(Modified, Kind::Function, "b")]
        );
        // A comment after a blank line is the top level's.
        let after = "fn a() {}\n// loose\n\nfn b() {}\n";
        let before = "fn a() {}\n\nfn b() {}\n";
        assert_eq!(
            changes("a.rs", before, after),
            vec![c(Modified, Kind::TopLevel, "")]
        );
    }

    #[test]
    fn a_trailing_comment_stays_with_the_line_it_ends() {
        let before = "fn a() {} // one\nfn b() {}\n";
        let after = "fn a() {} // two\nfn b() {}\n";
        assert_eq!(
            changes("a.rs", before, after),
            vec![c(Modified, Kind::TopLevel, "")]
        );
    }

    #[test]
    fn several_impl_blocks_pair_by_content() {
        let before = "impl A {\n    fn a() {}\n}\nimpl A {\n    fn b() {}\n}\n";
        let after = "impl A {\n    fn b() {}\n}\n";
        assert_eq!(
            changes("a.rs", before, after),
            vec![c(Removed, Kind::Impl, "A")]
        );
    }

    #[test]
    fn trait_impls_are_named_by_trait_and_type() {
        let after = "impl Display for Task {\n    fn fmt(&self) {}\n}\n";
        assert_eq!(
            changes("a.rs", "", after),
            vec![c(Added, Kind::Impl, "<Task as Display>")]
        );
    }

    #[test]
    fn csharp_members() {
        let before = "namespace N {\npublic class C {\n  private int _x, _y;\n  public string Name { get; set; }\n  public C() {}\n  public void Run(int a) {}\n}\n}\n";
        let after = "namespace N {\npublic class C {\n  private int _x, _y;\n  public string Name { get; init; }\n  public C() { Init(); }\n  public void Run(int a) {}\n  public event EventHandler Changed;\n}\n}\n";
        assert_eq!(
            changes("C.cs", before, after),
            vec![
                c(Modified, Kind::Property, "N.C.Name"),
                c(Modified, Kind::Constructor, "N.C.C"),
                c(Added, Kind::Event, "N.C.Changed"),
            ]
        );
    }

    #[test]
    fn csharp_overloads_pair_by_content_then_order() {
        let before = "class C {\n  void Run(int a) {}\n  void Run(string s) {}\n}\n";
        let after =
            "class C {\n  void Run(int a) {}\n  void Run(long l) {}\n  void Run(string s) {}\n}\n";
        assert_eq!(
            changes("C.cs", before, after),
            vec![c(Added, Kind::Method, "C.Run")]
        );
    }

    #[test]
    fn typescript_exports_and_class_members() {
        let before = "import x from \"y\";\nexport function f(a: number) { return a; }\nexport class C {\n  private x = 1;\n  run(): void {}\n}\nexport const MAX = 1;\n";
        let after = "import x from \"y\";\nfunction f(a: number) { return a; }\nexport class C {\n  private x = 2;\n  run(): void {}\n  stop(): void {}\n}\nexport const MAX = 1;\n";
        assert_eq!(
            changes("a.ts", before, after),
            vec![
                c(Modified, Kind::Function, "f"),
                c(Modified, Kind::Field, "C.x"),
                c(Added, Kind::Method, "C.stop"),
            ]
        );
    }

    #[test]
    fn locals_are_part_of_their_function() {
        let before = "function f() {\n  const a = 1;\n  return a;\n}\n";
        let after = "function f() {\n  const a = 2;\n  return a;\n}\n";
        assert_eq!(
            changes("a.js", before, after),
            vec![c(Modified, Kind::Function, "f")]
        );
    }

    #[test]
    fn tsx_and_javascript_fields() {
        assert_eq!(
            changes("App.tsx", "", "export function App() { return <div/>; }\n"),
            vec![c(Added, Kind::Function, "App")]
        );
        let before = "class C { x = 1; m() {} }\n";
        let after = "class C { x = 1; #p = 2; m() {} }\n";
        assert_eq!(
            changes("a.js", before, after),
            vec![c(Added, Kind::Field, "C.#p")]
        );
    }

    #[test]
    fn python_methods_fields_and_decorators() {
        let before = "import os\nMAX = 1\nclass C(B):\n    x: int = 1\n    def run(self):\n        y = 1\n\n@dec\ndef g():\n    pass\n";
        let after = "import os\nMAX = 2\nclass C(B):\n    x: int = 1\n    def run(self):\n        y = 2\n\n@dec(1)\ndef g():\n    pass\n";
        assert_eq!(
            changes("a.py", before, after),
            vec![
                c(Modified, Kind::Variable, "MAX"),
                c(Modified, Kind::Method, "C.run"),
                c(Modified, Kind::Function, "g"),
            ]
        );
    }

    #[test]
    fn go_types_methods_and_fields() {
        let before = "package p\n\ntype Task struct {\n\tID string\n}\n\nfunc (t *Task) Commit() error { return nil }\n\nconst MAX = 1\n";
        let after = "package p\n\ntype Task struct {\n\tID string\n\tHome string\n}\n\nfunc (t *Task) Commit() error { return t.x() }\n\nconst MAX = 1\n";
        assert_eq!(
            changes("task.go", before, after),
            vec![
                c(Added, Kind::Field, "Task.Home"),
                c(Modified, Kind::Method, "Task.Commit"),
            ]
        );
    }

    #[test]
    fn a_file_of_syntax_errors_still_has_a_top_level() {
        assert_eq!(
            changes("a.rs", "fn a() {}\n", "fn a() {}\n}}} ((\n"),
            vec![c(Modified, Kind::TopLevel, "")]
        );
    }

    #[test]
    fn deep_nesting_does_not_overflow() {
        let deep = format!(
            "fn a() {{ {}1{} }}\n",
            "(".repeat(50_000),
            ")".repeat(50_000)
        );
        let outline = crate::diff("a.rs", None, Some(deep.as_bytes()));
        assert!(matches!(
            outline,
            crate::Outline::Changes { .. } | crate::Outline::Skipped { .. }
        ));
    }
}
