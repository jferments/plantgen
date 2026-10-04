//! Module strings and derivation steps.
//!
//! One derivation step rewrites every module of the string in parallel with
//! its first matching production, then expands every new or unchanged module
//! with decomposition rules, then applies cuts (`%`). Each module carries a
//! [`Lineage`] (see [`crate::rng`]) and its birth time.

use super::expr::{Code, EnvValues, NO_ENV, Scope, eval, truthy};
use super::lexer::Span;
use super::program::{CUT, CompiledRule, FIRST_USER_SYMBOL, Item, MOVE, POP, PUSH, Program};
use super::{GrowthError, Limits};
use crate::rng::{Lineage, combine, hash_words, unit};

/// One module in a string. Its parameters live in [`ModuleString::params`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Module {
    pub symbol: u16,
    pub arity: u8,
    pub params_at: u32,
    pub lineage: Lineage,
    /// Time at which the module first existed, in years.
    pub born: f64,
}

/// A string of modules with a shared parameter arena.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModuleString {
    pub modules: Vec<Module>,
    pub params: Vec<f64>,
}

impl ModuleString {
    #[must_use]
    pub fn params(&self, module: &Module) -> &[f64] {
        let start = module.params_at as usize;
        &self.params[start..start + usize::from(module.arity)]
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }
}

/// Time of the string being rewritten.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clock {
    pub step: u32,
    pub t: f64,
    pub dt: f64,
}

/// Decomposition children use their own half of each ordinal space, so they
/// never collide with production children of the same module and step.
const DECOMPOSED: u32 = 1 << 30;
const SALT_CHOICE: u64 = 0x6368_6f69;
const SALT_AXIOM: u64 = 0x6178_696f;
const SALT_DECOMPOSE: u64 = 0x6465_636f;

/// Appends modules to a new string, applying cuts and dropping empty
/// branches as it goes.
struct Writer<'l> {
    out: ModuleString,
    /// While cutting: how many branches deep inside the cut branch we are.
    skipping: Option<u32>,
    limits: &'l Limits,
    program: &'l Program,
}

impl Writer<'_> {
    /// A turtle command that only changes turtle state and that no rule
    /// rewrites: a branch holding nothing else has no effect.
    fn inert(&self, symbol: u16) -> bool {
        let index = usize::from(symbol);
        (MOVE..FIRST_USER_SYMBOL).contains(&symbol)
            && self.program.productions[index].is_empty()
            && self.program.decompositions[index].is_empty()
            && self.program.interpretations[index].is_empty()
    }

    fn push(
        &mut self,
        symbol: u16,
        params: &[f64],
        lineage: Lineage,
        born: f64,
    ) -> Result<(), GrowthError> {
        if let Some(depth) = self.skipping {
            match symbol {
                PUSH => self.skipping = Some(depth + 1),
                POP if depth == 0 => self.skipping = None,
                POP => self.skipping = Some(depth - 1),
                _ => {}
            }
            if self.skipping.is_some() {
                return Ok(());
            }
        }
        if symbol == CUT {
            self.skipping = Some(0);
            return Ok(());
        }
        if symbol == POP {
            // A branch of turtle-state commands alone (or nothing) changes
            // nothing, so it is dropped: for example `[ /(30) ]` left behind
            // when the organ it turned toward is shed.
            let mut at = self.out.modules.len();
            while at > 0 && self.inert(self.out.modules[at - 1].symbol) {
                at -= 1;
            }
            if at > 0 && self.out.modules[at - 1].symbol == PUSH {
                let params_at = self.out.modules[at - 1].params_at as usize;
                self.out.params.truncate(params_at);
                self.out.modules.truncate(at - 1);
                return Ok(());
            }
        }
        if self.out.modules.len() >= self.limits.max_modules {
            return Err(GrowthError::Limit {
                what: "modules in the string",
                limit: self.limits.max_modules as u64,
            });
        }
        let params_at = u32::try_from(self.out.params.len()).map_err(|_| GrowthError::Limit {
            what: "parameters in the string",
            limit: u64::from(u32::MAX),
        })?;
        self.out.params.extend_from_slice(params);
        self.out.modules.push(Module {
            symbol,
            arity: u8::try_from(params.len()).unwrap_or(u8::MAX),
            params_at,
            lineage,
            born,
        });
        Ok(())
    }
}

/// Rewrites strings for one program, parameter set and seed.
pub struct Deriver<'a> {
    program: &'a Program,
    globals: &'a [f64],
    limits: &'a Limits,
    stack: Vec<f64>,
}

impl<'a> Deriver<'a> {
    #[must_use]
    pub fn new(program: &'a Program, globals: &'a [f64], limits: &'a Limits) -> Self {
        Self {
            program,
            globals,
            limits,
            stack: Vec::with_capacity(32),
        }
    }

    /// The axiom, decomposed, at time 0.
    ///
    /// # Errors
    ///
    /// Fails if a parameter is not finite or a limit is exceeded.
    pub fn axiom(&mut self, seed: u64, dt: f64) -> Result<ModuleString, GrowthError> {
        let mut writer = Writer {
            out: ModuleString::default(),
            skipping: None,
            limits: self.limits,
            program: self.program,
        };
        let clock = Clock {
            step: 0,
            t: 0.0,
            dt,
        };
        let scope = Scope {
            globals: self.globals,
            locals: &[],
            env: &NO_ENV,
            t: 0.0,
            dt,
            age: 0.0,
            step: 0.0,
            key: hash_words(&[seed, SALT_AXIOM]),
        };
        let program = self.program;
        let mut args = Vec::new();
        for item in &*program.axiom {
            self.eval_args(item, &scope, &mut args, Span::default())?;
            let lineage = Lineage::root(seed, item.ordinal);
            self.emit(&mut writer, item.symbol, &args, lineage, 0.0, clock, 0.0, 0)?;
        }
        Ok(writer.out)
    }

    /// One derivation step. `env` lists, in string order, the environment
    /// values of the modules that declared queries.
    ///
    /// # Errors
    ///
    /// Fails if a rule produces a non-finite parameter or a limit is exceeded.
    pub fn derive(
        &mut self,
        input: &ModuleString,
        env: &[(u32, EnvValues)],
        clock: Clock,
    ) -> Result<ModuleString, GrowthError> {
        let program = self.program;
        let mut writer = Writer {
            out: ModuleString {
                modules: Vec::with_capacity(input.modules.len() + input.modules.len() / 4),
                params: Vec::with_capacity(input.params.len() + input.params.len() / 4),
            },
            skipping: None,
            limits: self.limits,
            program: self.program,
        };
        let next_t = clock.t + clock.dt;
        let mut cursor = 0;
        let mut args = Vec::new();
        for (index, module) in input.modules.iter().enumerate() {
            let module_env = match env.get(cursor) {
                Some((at, values)) if *at as usize == index => {
                    cursor += 1;
                    values
                }
                _ => &NO_ENV,
            };
            let locals = input.params(module);
            let rules = &program.productions[usize::from(module.symbol)];
            let scope = Scope {
                globals: self.globals,
                locals,
                env: module_env,
                t: clock.t,
                dt: clock.dt,
                age: clock.t - module.born,
                step: f64::from(clock.step),
                key: combine(module.lineage.0, u64::from(clock.step)),
            };
            let Some(rule) = self.choose(rules, &scope)? else {
                self.emit(
                    &mut writer,
                    module.symbol,
                    locals,
                    module.lineage,
                    module.born,
                    clock,
                    next_t,
                    0,
                )?;
                continue;
            };
            for (position, item) in rule.successor.iter().enumerate() {
                self.eval_args(item, &scope, &mut args, rule.span)?;
                let (lineage, born) = if rule.continuation == Some(position) {
                    (module.lineage, module.born)
                } else {
                    (module.lineage.child(clock.step, item.ordinal), next_t)
                };
                self.emit(
                    &mut writer,
                    item.symbol,
                    &args,
                    lineage,
                    born,
                    clock,
                    next_t,
                    0,
                )?;
            }
        }
        Ok(writer.out)
    }

    /// Write a module, expanding it with decomposition rules first.
    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        writer: &mut Writer<'_>,
        symbol: u16,
        params: &[f64],
        lineage: Lineage,
        born: f64,
        clock: Clock,
        now: f64,
        depth: usize,
    ) -> Result<(), GrowthError> {
        let program = self.program;
        let rules = &program.decompositions[usize::from(symbol)];
        if rules.is_empty() {
            return writer.push(symbol, params, lineage, born);
        }
        let scope = Scope {
            globals: self.globals,
            locals: params,
            env: &NO_ENV,
            t: now,
            dt: clock.dt,
            age: now - born,
            step: f64::from(clock.step),
            key: hash_words(&[lineage.0, u64::from(clock.step), SALT_DECOMPOSE]),
        };
        let Some(rule) = self.choose(rules, &scope)? else {
            return writer.push(symbol, params, lineage, born);
        };
        if depth >= self.limits.max_decomposition_depth {
            return Err(GrowthError::Rule {
                span: rule.span,
                message: format!(
                    "decomposition is nested more than {} deep",
                    self.limits.max_decomposition_depth
                ),
            });
        }
        let mut args = Vec::new();
        for item in &*rule.successor {
            self.eval_args(item, &scope, &mut args, rule.span)?;
            let child = lineage.child(clock.step, item.ordinal | DECOMPOSED);
            self.emit(
                writer,
                item.symbol,
                &args,
                child,
                born,
                clock,
                now,
                depth + 1,
            )?;
        }
        Ok(())
    }

    fn eval_args(
        &mut self,
        item: &Item,
        scope: &Scope<'_>,
        args: &mut Vec<f64>,
        span: Span,
    ) -> Result<(), GrowthError> {
        args.clear();
        for code in &*item.args {
            let value = eval(code, scope, &mut self.stack);
            if !value.is_finite() {
                return Err(GrowthError::Rule {
                    span,
                    message: format!(
                        "a parameter of `{}` evaluated to {value}",
                        self.program.symbols[usize::from(item.symbol)].name
                    ),
                });
            }
            args.push(value);
        }
        Ok(())
    }

    /// The rule to apply: see [`choose`].
    ///
    /// # Errors
    ///
    /// Fails if a weight is not finite.
    pub fn choose<'r>(
        &mut self,
        rules: &'r [CompiledRule],
        scope: &Scope<'_>,
    ) -> Result<Option<&'r CompiledRule>, GrowthError> {
        choose(rules, scope, &mut self.stack)
    }
}

fn condition(rule: &CompiledRule, scope: &Scope<'_>, stack: &mut Vec<f64>) -> bool {
    rule.condition
        .as_ref()
        .is_none_or(|code| truthy(eval(code, scope, stack)))
}

fn weight(
    code: &Code,
    scope: &Scope<'_>,
    span: Span,
    stack: &mut Vec<f64>,
) -> Result<f64, GrowthError> {
    let weight = eval(code, scope, stack);
    if weight.is_nan() || weight.is_infinite() {
        return Err(GrowthError::Rule {
            span,
            message: format!("a rule weight evaluated to {weight}"),
        });
    }
    Ok(weight.max(0.0))
}

/// The first rule whose condition holds. A weighted rule competes with the
/// weighted rules that directly follow it; one of those whose condition holds
/// is drawn in proportion to its weight, keyed by `scope.key`.
///
/// # Errors
///
/// Fails if a weight is not finite.
pub fn choose<'r>(
    rules: &'r [CompiledRule],
    scope: &Scope<'_>,
    stack: &mut Vec<f64>,
) -> Result<Option<&'r CompiledRule>, GrowthError> {
    let mut first = 0;
    while first < rules.len() {
        let rule = &rules[first];
        if !condition(rule, scope, stack) {
            first += 1;
            continue;
        }
        if rule.weight.is_none() {
            return Ok(Some(rule));
        }
        let mut end = first + 1;
        while end < rules.len() && rules[end].weight.is_some() {
            end += 1;
        }
        let mut weights = Vec::with_capacity(end - first);
        for (offset, candidate) in rules[first..end].iter().enumerate() {
            let open = offset == 0 || condition(candidate, scope, stack);
            let value = match (&candidate.weight, open) {
                (Some(code), true) => weight(code, scope, candidate.span, stack)?,
                _ => 0.0,
            };
            weights.push(value);
        }
        let total: f64 = weights.iter().sum();
        if total > 0.0 {
            let draw = unit(hash_words(&[scope.key, SALT_CHOICE, first as u64])) * total;
            let mut cumulative = 0.0;
            for (offset, value) in weights.iter().enumerate() {
                cumulative += value;
                if draw < cumulative {
                    return Ok(Some(&rules[first + offset]));
                }
            }
            // Rounding can leave the draw at the very top of the range.
            let last = weights.iter().rposition(|value| *value > 0.0).unwrap_or(0);
            return Ok(Some(&rules[first + last]));
        }
        first = end;
    }
    Ok(None)
}

#[cfg(test)]
// Tests check exact results: clamped, integral and copied values.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn run(source: &str, steps: u32) -> (Program, ModuleString) {
        let program = Program::compile(source).unwrap();
        let globals = program.resolve_params(&BTreeMap::new()).unwrap();
        let limits = Limits::default();
        let mut deriver = Deriver::new(&program, &globals, &limits);
        let mut string = deriver.axiom(7, 1.0).unwrap();
        for step in 0..steps {
            let clock = Clock {
                step,
                t: f64::from(step),
                dt: 1.0,
            };
            string = deriver.derive(&string, &[], clock).unwrap();
        }
        (program, string)
    }

    fn text(program: &Program, string: &ModuleString) -> String {
        string
            .modules
            .iter()
            .map(|module| program.symbols[usize::from(module.symbol)].name.as_str())
            .collect::<Vec<_>>()
            .join("")
    }

    #[test]
    fn algae_grows_like_fibonacci() {
        let (program, string) = run(
            "lsystem algae 1; module A; module B; axiom A; rule A -> A B; rule B -> A;",
            7,
        );
        assert_eq!(string.len(), 34);
        assert!(text(&program, &string).starts_with("ABAABABAABAAB"));
    }

    #[test]
    fn continuation_keeps_lineage_and_birth() {
        let (program, string) = run("lsystem p 1; module A; axiom A; rule A -> F [ A ] A;", 3);
        let a = program.symbol_id("A").unwrap();
        let apices: Vec<&Module> = string.modules.iter().filter(|m| m.symbol == a).collect();
        // The last A is the original apex, continued every step.
        let root = Lineage::root(7, 0);
        assert_eq!(apices.last().unwrap().lineage, root);
        assert_eq!(apices.last().unwrap().born, 0.0);
        // Every module has a distinct lineage except turtle commands.
        let mut lineages: Vec<u64> = string
            .modules
            .iter()
            .filter(|m| m.symbol >= 3)
            .map(|m| m.lineage.0)
            .collect();
        lineages.sort_unstable();
        lineages.dedup();
        assert_eq!(
            lineages.len(),
            string.modules.iter().filter(|m| m.symbol >= 3).count()
        );
    }

    #[test]
    fn cut_removes_the_rest_of_the_branch_and_empty_branches_vanish() {
        let (program, string) = run(
            "lsystem p 1; module A; module S; axiom F [ S F A ] [ F A ] F;
             rule S -> %;",
            1,
        );
        assert_eq!(text(&program, &string), "F[FA]F");
    }

    #[test]
    fn branches_of_turtle_state_alone_vanish_unless_a_rule_rewrites_them() {
        let (program, string) = run(
            "lsystem p 1; module L; axiom F [ /(30) L ] [ +(10) [ &(5) ] F ] F;
             rule L -> ;",
            1,
        );
        assert_eq!(text(&program, &string), "F[+F]F");
        let (program, string) = run(
            "lsystem p 1; module L; axiom F [ /(30) L ] F; rule L -> ; rule /(a) -> /(a);",
            1,
        );
        assert_eq!(text(&program, &string), "F[/]F");
    }

    #[test]
    fn decomposition_expands_counts_in_one_step() {
        let (program, string) = run(
            "lsystem p 1; module W(n); module B;
             axiom W(4);
             decompose W(n) : n > 0 -> [ B ] W(n - 1);
             decompose W(n) -> ;",
            0,
        );
        assert_eq!(text(&program, &string), "[B][B][B][B]");
    }

    #[test]
    fn runaway_decomposition_is_stopped() {
        let program =
            Program::compile("lsystem p 1; module W; axiom W; decompose W -> F W;").unwrap();
        let limits = Limits::default();
        let mut deriver = Deriver::new(&program, &[], &limits);
        let error = deriver.axiom(1, 1.0).unwrap_err();
        assert!(error.to_string().contains("nested"), "{error}");
    }

    #[test]
    fn module_limit_is_enforced() {
        let program = Program::compile("lsystem p 1; module A; axiom A; rule A -> A A;").unwrap();
        let limits = Limits {
            max_modules: 100,
            ..Limits::default()
        };
        let mut deriver = Deriver::new(&program, &[], &limits);
        let mut string = deriver.axiom(1, 1.0).unwrap();
        let mut result = Ok(());
        for step in 0..10 {
            let clock = Clock {
                step,
                t: f64::from(step),
                dt: 1.0,
            };
            match deriver.derive(&string, &[], clock) {
                Ok(next) => string = next,
                Err(error) => {
                    result = Err(error);
                    break;
                }
            }
        }
        assert!(matches!(result, Err(GrowthError::Limit { .. })));
    }

    #[test]
    fn weighted_rules_split_by_weight_and_are_reproducible() {
        let source = "lsystem p 1; module A; module X; module Y;
             axiom A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A
                   A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A A;
             rule A weight 3 -> X;
             rule A weight 1 -> Y;";
        let (program, first) = run(source, 1);
        let (_, second) = run(source, 1);
        assert_eq!(first, second);
        let x = program.symbol_id("X").unwrap();
        let count = first.modules.iter().filter(|m| m.symbol == x).count();
        assert!(
            (45..=75).contains(&count),
            "{count} of 80 chose the weight-3 rule"
        );
    }
}
