//! perf/bun-gap (PoC, not for upstream as is): skip function bodies that the previous
//! peephole pass left untouched.
//!
//! `OXC_BG_SKIP_CLEAN=1`. Late passes change a handful of nodes yet walk the whole
//! program. A pass now records the nearest function scope of every AST change, and the
//! symbols whose facts changed (a reference removed or minted, the cached value, the
//! function summary, the member-write effect, or newly proved dead). The next pass walks
//! only those functions, the functions that declare or reference those symbols, their
//! ancestors, and the program's top level. Every other function body is taken out of
//! the tree for the duration of the walk and put back on exit.
//!
//! When a partial pass makes no change, one full pass confirms the fixed point
//! (`OXC_BG_SKIP_NO_CONFIRM=1` skips that). Bodies inside a class are never skipped,
//! because private-member usage is collected while walking them. Only full minify
//! skips: Rolldown runs tree-shake-only DCE on every module, where the bookkeeping
//! costs more than it saves.

use std::{
    hash::{Hash, Hasher},
    sync::OnceLock,
};

use rustc_hash::{FxHashSet, FxHasher};

use oxc_allocator::ArenaVec;
use oxc_ast::ast::*;
use oxc_syntax::{reference::ReferenceId, scope::ScopeId, symbol::SymbolId};

use crate::{TraverseCtx, bg_stats::enclosing_fn};

pub fn enabled() -> bool {
    static F: OnceLock<bool> = OnceLock::new();
    *F.get_or_init(|| std::env::var_os("OXC_BG_SKIP_CLEAN").is_some_and(|v| v != "0"))
}

fn confirm() -> bool {
    static F: OnceLock<bool> = OnceLock::new();
    *F.get_or_init(|| !std::env::var_os("OXC_BG_SKIP_NO_CONFIRM").is_some_and(|v| v != "0"))
}

#[derive(Default)]
pub struct SkipState<'a> {
    /// Function scopes the current pass walks; `None` walks everything.
    visit: Option<FxHashSet<ScopeId>>,
    /// The set for the next pass, computed when this one completes.
    pending: Option<FxHashSet<ScopeId>>,
    /// One entry per entered function body: the statements taken out, if skipped.
    stash: Vec<Option<ArenaVec<'a, Statement<'a>>>>,
    changed_fns: FxHashSet<ScopeId>,
    /// Outermost function bodies skipped by the current pass.
    skipped_fns: FxHashSet<ScopeId>,
    /// Per symbol: hash of the facts the last pass that computed them saw.
    value_hash: Vec<u64>,
    fact_hash: Vec<u64>,
    refs_len_at_start: usize,
    pub skipped: u32,
    pub passes: u32,
    pub partial_passes: u32,
}

impl<'a> SkipState<'a> {
    pub fn record_change(&mut self, function_scope: ScopeId) {
        self.changed_fns.insert(function_scope);
    }
}

pub fn begin_pass(ctx: &mut TraverseCtx<'_>) {
    let refs_len = ctx.scoping().references_len();
    let Some(skip) = ctx.state.bg_skip.as_mut() else { return };
    skip.visit = skip.pending.take();
    skip.skipped_fns.clear();
    skip.refs_len_at_start = refs_len;
    skip.passes += 1;
    if skip.visit.is_some() {
        skip.partial_passes += 1;
    }
}

pub fn enter_function_body<'a>(body: &mut FunctionBody<'a>, ctx: &mut TraverseCtx<'a>) {
    let scope = ctx.current_scope_id();
    let in_class = !ctx.state.private_member_usage.is_at_root();
    let empty = ArenaVec::new_in(ctx);
    let Some(skip) = ctx.state.bg_skip.as_mut() else { return };
    if !in_class && skip.visit.as_ref().is_some_and(|v| !v.contains(&scope)) {
        skip.stash.push(Some(std::mem::replace(&mut body.statements, empty)));
        skip.skipped_fns.insert(scope);
        skip.skipped += 1;
    } else {
        skip.stash.push(None);
    }
}

pub fn exit_function_body<'a>(body: &mut FunctionBody<'a>, ctx: &mut TraverseCtx<'a>) {
    let Some(skip) = ctx.state.bg_skip.as_mut() else { return };
    if let Some(Some(statements)) = skip.stash.pop() {
        body.statements = statements;
    }
}

fn hash_one(value: impl Hash) -> u64 {
    let mut h = FxHasher::default();
    value.hash(&mut h);
    h.finish()
}

/// Complete a pass: compute what the next pass walks. Returns whether another pass is
/// needed, given the ordinary fixed-point decision.
pub fn end_pass(
    ctx: &mut TraverseCtx<'_>,
    removed_symbols: &[SymbolId],
    needs_another_pass: bool,
) -> bool {
    let Some(mut skip) = ctx.state.bg_skip.take() else { return needs_another_pass };
    let was_partial = skip.visit.is_some();
    let next = next_visit_set(ctx, &mut skip, removed_symbols);
    let result = if needs_another_pass {
        skip.pending = next;
        true
    } else if !was_partial {
        false
    } else if next.as_ref().is_some_and(|v| v.iter().any(|s| skip.skipped_fns.contains(s))) {
        // No AST change, but a fact computed during this pass (e.g. a constant that became
        // inlinable) can unlock a rule in a body this pass skipped; a full pass would have
        // applied it later in the same walk.
        skip.pending = next;
        true
    } else if confirm() {
        // A full pass confirms the fixed point.
        skip.pending = None;
        true
    } else {
        false
    };
    skip.changed_fns.clear();
    ctx.state.bg_skip = Some(skip);
    result
}

fn next_visit_set(
    ctx: &TraverseCtx<'_>,
    skip: &mut SkipState<'_>,
    removed_symbols: &[SymbolId],
) -> Option<FxHashSet<ScopeId>> {
    let scoping = ctx.scoping();
    let symbols = &ctx.state.symbols;
    let len = scoping.symbols_len();
    let first = skip.fact_hash.is_empty();
    if first {
        skip.value_hash.resize(len, 0);
        skip.fact_hash.resize(len, 0);
    }
    let mut changed: FxHashSet<SymbolId> = removed_symbols.iter().copied().collect();
    for index in skip.refs_len_at_start..scoping.references_len() {
        if let Some(symbol_id) = scoping.get_reference(ReferenceId::from_usize(index)).symbol_id() {
            changed.insert(symbol_id);
        }
    }
    for index in 0..len {
        let symbol_id = SymbolId::from_usize(index);
        // A symbol declared in a skipped body has no value this pass; keep the last one.
        if let Some(value) = symbols.value(symbol_id) {
            skip.value_hash[index] = value.bg_hash();
        }
        let fact = hash_one((
            skip.value_hash[index],
            symbols.function_summary(symbol_id) as u8,
            symbols.member_write_effect(symbol_id) as u8,
            symbols.function_is_dead(symbol_id),
        ));
        if first || fact != skip.fact_hash[index] {
            skip.fact_hash[index] = fact;
            changed.insert(symbol_id);
        }
    }

    if first {
        // Nothing to compare the first pass's facts with: walk everything next.
        return None;
    }
    let mut seeds: Vec<ScopeId> = skip.changed_fns.iter().copied().collect();
    for symbol_id in changed {
        seeds.push(enclosing_fn(scoping, scoping.symbol_scope_id(symbol_id)));
        for &reference_id in scoping.get_resolved_reference_ids(symbol_id) {
            seeds.push(enclosing_fn(scoping, scoping.get_reference(reference_id).scope_id()));
        }
    }
    let mut visit = FxHashSet::default();
    for seed in seeds {
        for scope in scoping.scope_ancestors(seed) {
            if scoping.scope_flags(scope).is_function() && !visit.insert(scope) {
                break;
            }
        }
    }
    Some(visit)
}
