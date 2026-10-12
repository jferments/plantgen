//! Module strings and derivation steps.
//!
//! One derivation step rewrites every module of the string in parallel with
//! its first matching production, then expands every new or unchanged module
//! with decomposition rules, then applies cuts (`%`). Each module carries a
//! [`Lineage`] (see [`crate::rng`]) and its birth time.

use std::ops::Range;

use super::expr::{Code, EnvValues, NO_ENV, Scope, eval, truthy};
use super::lexer::Span;
use super::program::{
    CUT, CompiledRule, FIRST_USER_SYMBOL, Item, MAX_RING, MOVE, POP, PUSH, Program, RING_SHIFT,
    ROLL, Ring, TURTLE_ORDINAL,
};
use super::{GrowthError, Limits};
use crate::cores::{self, Lease};
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
const SALT_RING: u64 = 0x7269_6e67;

/// The ordinal of the turn a ring writes after copy k's `[` (with k <<
/// `RING_SHIFT` added): a turtle ordinal no compiled item has.
const RING_TURN: u32 = TURTLE_ORDINAL | ((1 << RING_SHIFT) - 1);

/// Where rewriting puts modules: the new string ([`Writer`]), or a chunk's
/// record when chunks of the string are rewritten side by side ([`Raw`]).
trait Sink {
    fn push(
        &mut self,
        symbol: u16,
        params: &[f64],
        lineage: Lineage,
        born: f64,
    ) -> Result<(), GrowthError>;
}

/// A chunk's modules as its rules wrote them, before cuts and empty
/// branches: the writer takes them in string order afterwards.
struct Raw {
    out: ModuleString,
    /// The most modules a chunk records; past it the chunk stops (`full`)
    /// and the step is rewritten in order instead.
    cap: usize,
    full: bool,
}

impl Sink for Raw {
    fn push(
        &mut self,
        symbol: u16,
        params: &[f64],
        lineage: Lineage,
        born: f64,
    ) -> Result<(), GrowthError> {
        if self.out.modules.len() >= self.cap {
            self.full = true;
            return Err(GrowthError::Limit {
                what: "modules in the string",
                limit: self.cap as u64,
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

/// Strings shorter than this are rewritten on one thread.
const PARALLEL_MODULES: usize = 20_000;

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
}

impl Sink for Writer<'_> {
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
        let mut stack = std::mem::take(&mut self.stack);
        let result = self.write(
            &program.axiom,
            &program.axiom_rings,
            &scope,
            Span::default(),
            &mut args,
            &mut stack,
            |symbol, args, ordinal, _, stack| {
                let lineage = Lineage::root(seed, ordinal);
                self.emit(
                    &mut writer,
                    symbol,
                    args,
                    lineage,
                    0.0,
                    clock,
                    0.0,
                    0,
                    stack,
                )
            },
        );
        self.stack = stack;
        result?;
        Ok(writer.out)
    }

    /// One derivation step. `env` lists, in string order, the environment
    /// values of the modules that declared queries.
    ///
    /// A long string is rewritten in chunks side by side, each into its own
    /// record; the writer then takes the records in string order, so cuts,
    /// empty branches, limits and errors come out as they would in order.
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
        let lease = Lease::take(if input.len() >= PARALLEL_MODULES {
            cores::per_task()
        } else {
            1
        });
        self.derive_on(input, env, clock, lease.threads())
    }

    /// [`Deriver::derive`] on `threads` threads.
    fn derive_on(
        &mut self,
        input: &ModuleString,
        env: &[(u32, EnvValues)],
        clock: Clock,
        threads: usize,
    ) -> Result<ModuleString, GrowthError> {
        let mut writer = Writer {
            out: ModuleString {
                modules: Vec::with_capacity(input.modules.len() + input.modules.len() / 4),
                params: Vec::with_capacity(input.params.len() + input.params.len() / 4),
            },
            skipping: None,
            limits: self.limits,
            program: self.program,
        };
        if threads > 1 && !input.is_empty() {
            // More chunks than threads, so that slow chunks even out.
            let count = threads * 8;
            let size = input.len().div_ceil(count);
            let chunks: Vec<Range<usize>> = (0..input.len())
                .step_by(size)
                .map(|start| start..(start + size).min(input.len()))
                .collect();
            let this = &*self;
            let rewritten = cores::map(chunks, threads, &|range| {
                let mut raw = Raw {
                    out: ModuleString::default(),
                    cap: this.limits.max_modules,
                    full: false,
                };
                let mut stack = Vec::with_capacity(32);
                let result = this.rewrite(input, range, env, clock, &mut stack, &mut raw);
                (raw, result)
            });
            if !rewritten.iter().any(|(raw, _)| raw.full) {
                for (raw, result) in rewritten {
                    for module in &raw.out.modules {
                        writer.push(
                            module.symbol,
                            raw.out.params(module),
                            module.lineage,
                            module.born,
                        )?;
                    }
                    result?;
                }
                return Ok(writer.out);
            }
        }
        let mut stack = std::mem::take(&mut self.stack);
        let result = self.rewrite(input, 0..input.len(), env, clock, &mut stack, &mut writer);
        self.stack = stack;
        result?;
        Ok(writer.out)
    }

    /// Rewrite modules `range` of `input` into `sink`: each with its first
    /// matching production, expanded by decomposition rules.
    fn rewrite(
        &self,
        input: &ModuleString,
        range: Range<usize>,
        env: &[(u32, EnvValues)],
        clock: Clock,
        stack: &mut Vec<f64>,
        sink: &mut dyn Sink,
    ) -> Result<(), GrowthError> {
        let program = self.program;
        let next_t = clock.t + clock.dt;
        // `env` holds each querying module once, in string order.
        let mut cursor = env.partition_point(|(at, _)| (*at as usize) < range.start);
        let mut args = Vec::new();
        for index in range {
            let module = &input.modules[index];
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
            let Some(rule) = choose(rules, &scope, stack)? else {
                self.emit(
                    sink,
                    module.symbol,
                    locals,
                    module.lineage,
                    module.born,
                    clock,
                    next_t,
                    0,
                    stack,
                )?;
                continue;
            };
            self.write(
                &rule.successor,
                &rule.rings,
                &scope,
                rule.span,
                &mut args,
                stack,
                |symbol, args, ordinal, position, stack| {
                    let (lineage, born) = if position.is_some() && rule.continuation == position {
                        (module.lineage, module.born)
                    } else {
                        (module.lineage.child(clock.step, ordinal), next_t)
                    };
                    self.emit(sink, symbol, args, lineage, born, clock, next_t, 0, stack)
                },
            )?;
        }
        Ok(())
    }

    /// Write a module, expanding it with decomposition rules first.
    #[allow(clippy::too_many_arguments)]
    fn emit(
        &self,
        writer: &mut dyn Sink,
        symbol: u16,
        params: &[f64],
        lineage: Lineage,
        born: f64,
        clock: Clock,
        now: f64,
        depth: usize,
        stack: &mut Vec<f64>,
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
        let Some(rule) = choose(rules, &scope, stack)? else {
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
        self.write(
            &rule.successor,
            &rule.rings,
            &scope,
            rule.span,
            &mut args,
            stack,
            |symbol, args, ordinal, _, stack| {
                let child = lineage.child(clock.step, ordinal | DECOMPOSED);
                self.emit(
                    writer,
                    symbol,
                    args,
                    child,
                    born,
                    clock,
                    now,
                    depth + 1,
                    stack,
                )
            },
        )
    }

    /// Write a successor (or the axiom): each item with its arguments
    /// evaluated in `scope`, handed to `module` with its ordinal and, for
    /// an item written as itself, its position in `items`. A ring writes
    /// its copies in turn: copy 0 its items as they stand, copy k the
    /// same items drawing with `scope`'s key mixed with k, their ordinals
    /// with k << `RING_SHIFT` added, turned k * 360 / n round the heading,
    /// by a `/` after its `[` or on an organ's own roll; they have no
    /// position. `args` holds each item's arguments in turn.
    #[allow(clippy::too_many_arguments)]
    fn write<F>(
        &self,
        items: &[Item],
        rings: &[Ring],
        scope: &Scope<'_>,
        span: Span,
        args: &mut Vec<f64>,
        stack: &mut Vec<f64>,
        mut module: F,
    ) -> Result<(), GrowthError>
    where
        F: FnMut(u16, &[f64], u32, Option<usize>, &mut Vec<f64>) -> Result<(), GrowthError>,
    {
        let mut rings = rings.iter().peekable();
        let mut position = 0;
        while position < items.len() {
            let Some(ring) = rings.next_if(|ring| ring.at == position) else {
                let item = &items[position];
                self.eval_args(item, scope, args, span, stack)?;
                module(item.symbol, args, item.ordinal, Some(position), stack)?;
                position += 1;
                continue;
            };
            let held = &items[ring.at..ring.at + ring.len];
            let copies = ring_copies(ring, scope, span, stack)?;
            for copy in 0..copies {
                let (scope, offset) = if copy == 0 {
                    (*scope, 0)
                } else {
                    let key = hash_words(&[scope.key, SALT_RING, u64::from(copy)]);
                    (Scope { key, ..*scope }, copy << RING_SHIFT)
                };
                let turn = f64::from(copy) * 360.0 / f64::from(copies);
                for (at, item) in (ring.at..).zip(held) {
                    self.eval_args(item, &scope, args, span, stack)?;
                    if copy > 0 && ring.len == 1 {
                        // An organ with its own turn: [size, roll, pitch, ...].
                        args[1] += turn;
                    }
                    let own = (copy == 0).then_some(at);
                    module(item.symbol, args, item.ordinal + offset, own, stack)?;
                    if copy > 0 && item.symbol == PUSH && at == ring.at {
                        module(ROLL, &[turn], RING_TURN + offset, None, stack)?;
                    }
                }
            }
            position = ring.at + ring.len;
        }
        Ok(())
    }

    fn eval_args(
        &self,
        item: &Item,
        scope: &Scope<'_>,
        args: &mut Vec<f64>,
        span: Span,
        stack: &mut Vec<f64>,
    ) -> Result<(), GrowthError> {
        args.clear();
        for code in &*item.args {
            let value = eval(code, scope, stack);
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

/// How many copies `ring` writes in `scope`: its count rounded half away
/// from zero, none below 1.
///
/// # Errors
///
/// Fails if the count is not finite, or above [`MAX_RING`].
fn ring_copies(
    ring: &Ring,
    scope: &Scope<'_>,
    span: Span,
    stack: &mut Vec<f64>,
) -> Result<u32, GrowthError> {
    let count = eval(&ring.count, scope, stack);
    if !count.is_finite() {
        return Err(GrowthError::Rule {
            span,
            message: format!("the count of a ring evaluated to {count}"),
        });
    }
    let count = count.round();
    if count > f64::from(MAX_RING) {
        return Err(GrowthError::Limit {
            what: "copies in a ring",
            limit: u64::from(MAX_RING),
        });
    }
    // Checked above: a whole number no larger than MAX_RING.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(count.max(0.0) as u32)
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
    fn an_organ_lives_on_in_the_organ_it_becomes() {
        let source = |successor: &str| {
            format!(
                "lsystem p 1; organ flower flower; organ fruit fruit; axiom flower(1);
                 rule flower(s) -> {successor};"
            )
        };
        let fruit = |successor: &str| {
            let (program, string) = run(&source(successor), 1);
            let fruit = program.symbol_id("fruit").unwrap();
            let module = string.modules.iter().find(|m| m.symbol == fruit).unwrap();
            (module.lineage, module.born)
        };
        // The flower in the axiom is ordinal 0 of the root, born at 0.
        let flower = (Lineage::root(7, 0), 0.0);
        assert_eq!(fruit("fruit(s)"), flower);
        assert_eq!(fruit("F fruit(s) [ flower(s) ]"), flower);
        // In a branch, or beside the flower itself, it is a new organ.
        assert_ne!(fruit("[ fruit(s) ]").0, flower.0);
        assert_ne!(fruit("fruit(s) flower(s)").0, flower.0);
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

    /// Chunks rewritten side by side give the string rewriting in order
    /// gives: cuts, empty branches and lineages across chunk edges.
    #[test]
    fn chunks_side_by_side_rewrite_as_in_order() {
        let source = "
            lsystem cuts 1;
            module A(n);
            module B;
            axiom A(0);
            rule A(n) : n < 12 -> [ +(10) B ] A(n + 1) [ -(10) % B ] [ /(30) ] A(n + 2);
            rule B -> F(1) B;
        ";
        let program = Program::compile(source).unwrap();
        let globals = program.resolve_params(&BTreeMap::new()).unwrap();
        let limits = Limits::default();
        let mut deriver = Deriver::new(&program, &globals, &limits);
        let mut string = deriver.axiom(7, 1.0).unwrap();
        for step in 0..14 {
            let clock = Clock {
                step,
                t: f64::from(step),
                dt: 1.0,
            };
            let alone = deriver.derive_on(&string, &[], clock, 1).unwrap();
            let shared = deriver.derive_on(&string, &[], clock, 4).unwrap();
            assert_eq!(alone, shared, "step {step}");
            string = alone;
        }
        assert!(string.len() > 1000);
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

    /// A ring of one grows what its bracket alone grows. A ring of n
    /// writes n copies, copy k turned k * 360 / n round the heading, each
    /// drawing its own numbers, with lineages of its own.
    #[test]
    fn a_ring_writes_its_copies_round_the_heading() {
        let ringed = |count: &str| {
            format!(
                "lsystem p 1; module A(n); organ leaf leaf; axiom A({count});
                 rule A(n) -> F ring(n) [ &(30) leaf(uniform(0.5, 1)) ] A(n);"
            )
        };
        let bracket = "lsystem p 1; module A(n); organ leaf leaf; axiom A(1);
             rule A(n) -> F [ &(30) leaf(uniform(0.5, 1)) ] A(n);";
        assert_eq!(run(bracket, 3).1, run(&ringed("1"), 3).1);

        let (program, string) = run(&ringed("3"), 1);
        assert_eq!(text(&program, &string), "F[&leaf][/&leaf][/&leaf]A");
        let of = |symbol: u16| -> Vec<&Module> {
            string
                .modules
                .iter()
                .filter(|m| m.symbol == symbol)
                .collect()
        };
        let turns: Vec<f64> = of(ROLL).iter().map(|m| string.params(m)[0]).collect();
        assert_eq!(turns, [120.0, 240.0]);
        let leaves = of(program.symbol_id("leaf").unwrap());
        let (_, alone) = run(bracket, 1);
        let first = alone
            .modules
            .iter()
            .find(|m| m.symbol == leaves[0].symbol)
            .unwrap();
        assert_eq!(
            (leaves[0].lineage, string.params(leaves[0])),
            (first.lineage, alone.params(first))
        );
        let sizes: Vec<f64> = leaves.iter().map(|m| string.params(m)[0]).collect();
        assert!(sizes[0] != sizes[1] && sizes[1] != sizes[2] && sizes[0] != sizes[2]);
        let mut lineages: Vec<u64> = leaves.iter().map(|m| m.lineage.0).collect();
        lineages.dedup();
        assert_eq!(lineages.len(), 3);
    }

    /// A ring writes its count rounded half away from zero, none below 1,
    /// and stops growth above 64; an organ that carries its own turn is
    /// turned on its roll.
    #[test]
    fn a_ring_counts_whole_copies_up_to_its_limit() {
        let rolls = |count: &str| {
            let source =
                format!("lsystem p 1; organ leaf leaf; axiom F ring({count}) leaf(1, 10, 30);");
            let (program, string) = run(&source, 0);
            let leaf = program.symbol_id("leaf").unwrap();
            string
                .modules
                .iter()
                .filter(|m| m.symbol == leaf)
                .map(|m| string.params(m)[1])
                .collect::<Vec<f64>>()
        };
        assert_eq!(rolls("4"), [10.0, 100.0, 190.0, 280.0]);
        assert_eq!(rolls("2.5"), [10.0, 130.0, 250.0]);
        assert!(rolls("0.4").is_empty());
        assert!(rolls("-3").is_empty());
        let program =
            Program::compile("lsystem p 1; organ leaf leaf; axiom ring(65) leaf(1, 0, 30);")
                .unwrap();
        let limits = Limits::default();
        let error = Deriver::new(&program, &[], &limits)
            .axiom(1, 1.0)
            .unwrap_err();
        assert_eq!(
            error,
            GrowthError::Limit {
                what: "copies in a ring",
                limit: 64
            }
        );
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
