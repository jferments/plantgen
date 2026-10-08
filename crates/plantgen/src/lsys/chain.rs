//! Programs that build on programs (growth plan G1): `extends`.
//!
//! A program whose header reads `lsystem sapindaceae 1 extends broadleaf;`
//! builds on `broadleaf`. Its text is compiled as a chain: the program,
//! then the program it extends, then that one's parent, each with its own
//! header, in one source (the library joins them; see
//! `crate::library::Library::program`). [`resolve`] merges the chain into
//! one program, from the furthest ancestor down, so that each program:
//!
//! - **parameters**: replaces the value of a parameter its parent declares
//!   (in the parent's place, so the new value may use only the parameters
//!   declared before it there) and adds new ones after the parent's;
//! - **modules, organs and bodies**: adds new ones after the parent's; it
//!   may not declare one its parent declares;
//! - **tools**: replaces single settings of a tool its parent configures
//!   (the same tool and version) and adds new tools;
//! - **axiom**: replaces the parent's, or keeps it when it has none;
//! - **rules**: adds its rules, which are tried before the parent's for
//!   the same module (an L-system applies the first rule whose condition
//!   holds), so a program overrides a rule by writing its own and inherits
//!   the rest.
//!
//! Rules are compiled in the parent's order and then the program's, so the
//! numbered call sites of `rand`, `gauss` and `uniform` in the parent's
//! rules keep their numbers: a program's added rules never change the
//! draws of the rules it inherits. A program that extends none compiles
//! exactly as before.

use super::ProgramError;
use super::ast::ProgramAst;
use super::lexer::Span;

/// The most programs in one chain.
pub const MAX_DEPTH: usize = 8;

/// Merge a parsed chain (the program first, then its ancestors) into one
/// program named and numbered as the first.
///
/// # Errors
///
/// Fails when a program extends another than the next in the chain, the
/// last extends one that is not given, the chain is too deep, or a
/// program redeclares a module, organ or body or reconfigures a tool at
/// another version.
pub fn resolve(mut chain: Vec<ProgramAst>) -> Result<ProgramAst, ProgramError> {
    if chain.len() > MAX_DEPTH {
        return err(
            Span::default(),
            format!("a program may build on at most {} others", MAX_DEPTH - 1),
        );
    }
    for pair in chain.windows(2) {
        let (child, parent) = (&pair[0], &pair[1]);
        match &child.extends {
            Some((name, _)) if *name == parent.name => {}
            Some((name, span)) => {
                return err(
                    *span,
                    format!(
                        "program `{}` extends `{name}`, but the program after it is `{}`",
                        child.name, parent.name
                    ),
                );
            }
            None => {
                return err(
                    Span::default(),
                    format!(
                        "program `{}` extends nothing, but program `{}` follows it",
                        child.name, parent.name
                    ),
                );
            }
        }
    }
    let Some(last) = chain.last() else {
        return err(Span::default(), "no program".into());
    };
    if let Some((name, span)) = &last.extends {
        return err(
            *span,
            format!(
                "program `{}` extends `{name}`, which is not given (a library adds the programs a program extends)",
                last.name
            ),
        );
    }
    let Some(mut merged) = chain.pop() else {
        return err(Span::default(), "no program".into());
    };
    let mut depth = u8::try_from(chain.len()).unwrap_or(u8::MAX);
    for rule in &mut merged.rules {
        rule.depth = depth;
    }
    while let Some(child) = chain.pop() {
        depth -= 1;
        merged = merge(merged, child, depth)?;
    }
    Ok(merged)
}

fn merge(mut parent: ProgramAst, child: ProgramAst, depth: u8) -> Result<ProgramAst, ProgramError> {
    for param in child.params {
        match parent.params.iter_mut().find(|p| p.name == param.name) {
            Some(existing) => *existing = param,
            None => parent.params.push(param),
        }
    }
    for module in child.modules {
        if parent.modules.iter().any(|m| m.name == module.name) {
            return err(
                module.span,
                format!(
                    "module `{}` is declared by program `{}`, which this one extends",
                    module.name, parent.name
                ),
            );
        }
        parent.modules.push(module);
    }
    for organ in child.organs {
        if parent.organs.iter().any(|o| o.name == organ.name) {
            return err(
                organ.span,
                format!(
                    "organ `{}` is declared by program `{}`, which this one extends",
                    organ.name, parent.name
                ),
            );
        }
        parent.organs.push(organ);
    }
    for body in child.bodies {
        if parent.bodies.iter().any(|b| b.name == body.name) {
            return err(
                body.span,
                format!(
                    "body `{}` is declared by program `{}`, which this one extends",
                    body.name, parent.name
                ),
            );
        }
        parent.bodies.push(body);
    }
    for tool in child.tools {
        match parent.tools.iter_mut().find(|t| t.name == tool.name) {
            Some(existing) if existing.version != tool.version => {
                return err(
                    tool.span,
                    format!(
                        "tool `{}@{}` is configured at version {} by program `{}`, which this one extends",
                        tool.name, tool.version, existing.version, parent.name
                    ),
                );
            }
            Some(existing) => {
                for (key, value, span) in tool.settings {
                    match existing.settings.iter_mut().find(|(k, _, _)| *k == key) {
                        Some(setting) => *setting = (key, value, span),
                        None => existing.settings.push((key, value, span)),
                    }
                }
            }
            None => parent.tools.push(tool),
        }
    }
    if !child.axiom.is_empty() || child.axiom_span != Span::default() {
        parent.axiom = child.axiom;
        parent.axiom_span = child.axiom_span;
    }
    for mut rule in child.rules {
        rule.depth = depth;
        parent.rules.push(rule);
    }
    parent.name = child.name;
    parent.revision = child.revision;
    parent.extends = None;
    Ok(parent)
}

fn err<T>(span: Span, message: String) -> Result<T, ProgramError> {
    Err(ProgramError { span, message })
}

#[cfg(test)]
mod tests {
    use crate::lsys::Program;
    use crate::lsys::program::CompiledRule;

    const PARENT: &str = "lsystem base 3;
param size = 2;
param half = size / 2;
module A(n);
organ leaf leaf;
tool pipe@1 { exponent = 2, tip = 0.01 };
axiom A(1);
rule A(n) : n < 5 -> F(rand()) A(n + 1);
rule A(n) -> leaf(half);
";

    fn bodies(rules: &[CompiledRule]) -> Vec<(&[crate::lsys::program::Item], bool)> {
        rules
            .iter()
            .map(|rule| (&*rule.successor, rule.condition.is_some()))
            .collect()
    }

    fn compile(text: &str) -> Program {
        Program::compile(text).unwrap_or_else(|error| panic!("{error}"))
    }

    #[test]
    fn an_empty_child_compiles_as_its_parent() {
        let parent = compile(PARENT);
        let child = compile(&format!("lsystem leafy 1 extends base;\n{PARENT}"));
        assert_eq!((child.name.as_str(), child.revision), ("leafy", 1));
        assert_eq!(child.params, parent.params);
        assert_eq!(child.symbols, parent.symbols);
        assert_eq!(child.axiom, parent.axiom);
        assert_eq!(child.tools, parent.tools);
        for (a, b) in child.productions.iter().zip(&parent.productions) {
            assert_eq!(bodies(a), bodies(b));
        }
    }

    #[test]
    fn a_child_overrides_values_and_rules_and_adds_its_own() {
        let parent = compile(PARENT);
        let child = compile(&format!(
            "lsystem leafy 1 extends base;
param size = 4;
param extra = 1;
module B;
tool pipe@1 {{ tip = 0.02 }};
rule A(n) : n < 2 -> F(uniform(0, 1)) B A(n + 1);
rule B -> ;
{PARENT}"
        ));
        let names: Vec<&str> = child.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["size", "half", "extra"]);
        let values = child
            .resolve_params(&std::collections::BTreeMap::default())
            .unwrap();
        assert_eq!(values, [4.0, 2.0, 1.0]);
        let a = usize::from(child.symbol_id("A").unwrap());
        // The child's rule first, then the parent's two in their order.
        assert_eq!(child.productions[a].len(), 3);
        // The parent's `rand()` keeps its call site 0; the child's draw,
        // compiled after the rules it inherits, takes the next.
        let first_args = |rule: &CompiledRule| rule.successor[0].args.clone();
        assert_eq!(
            first_args(&child.productions[a][1]),
            first_args(&parent.productions[a][0])
        );
        assert!(format!("{:?}", first_args(&child.productions[a][0])).contains("Uniform(1)"));
        let pipe = crate::lsys::program::ToolKind::Pipe;
        assert_ne!(child.tool(pipe), parent.tool(pipe));
        assert_eq!(child.axiom, parent.axiom);
    }

    #[test]
    fn a_broken_chain_is_refused_with_its_reason() {
        let missing = Program::compile("lsystem leafy 1 extends base;\n").unwrap_err();
        assert!(
            missing.message.contains("`base`, which is not given"),
            "{missing}"
        );
        let wrong =
            Program::compile(&format!("lsystem leafy 1 extends other;\n{PARENT}")).unwrap_err();
        assert!(
            wrong.message.contains("the program after it is `base`"),
            "{wrong}"
        );
        let twice = Program::compile(&format!(
            "lsystem leafy 1 extends base;\nmodule A(n);\n{PARENT}"
        ))
        .unwrap_err();
        assert!(
            twice
                .message
                .contains("module `A` is declared by program `base`"),
            "{twice}"
        );
        let single =
            crate::lsys::parser::parse(&format!("lsystem leafy 1 extends base;\n{PARENT}"))
                .unwrap_err();
        assert!(single.message.contains("chain"), "{single}");
    }
}
