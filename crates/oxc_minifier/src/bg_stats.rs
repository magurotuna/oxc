//! perf/bun-gap (PoC instrumentation, not for upstream): per-pass compressor statistics.
//!
//! `OXC_BG_PASS_STATS=1` prints one line per peephole pass to stderr: traversal and
//! completion time, how many AST changes the pass made and where, and how many AST nodes
//! the next pass would have to visit if it skipped every function body that nothing in
//! this pass touched (directly, or through a symbol it declares or references).

use std::{
    fmt::Write as _,
    hash::{Hash, Hasher},
    panic::Location,
    sync::OnceLock,
    time::Duration,
};

use rustc_hash::{FxHashMap, FxHashSet, FxHasher};

use oxc_ast::{AstKind, ast::*};
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::Scoping;
use oxc_syntax::{
    scope::{ScopeFlags, ScopeId},
    symbol::SymbolId,
};

use crate::TraverseCtx;

pub fn enabled() -> bool {
    static F: OnceLock<bool> = OnceLock::new();
    *F.get_or_init(|| std::env::var_os("OXC_BG_PASS_STATS").is_some_and(|v| v != "0"))
}

#[derive(Default)]
pub struct PassStats {
    pass: u32,
    changes: u32,
    sites: FxHashMap<&'static Location<'static>, u32>,
    /// Nearest enclosing function scope (or the root scope) of every AST change this pass.
    changed_fns: FxHashSet<ScopeId>,
    prev_values: Vec<u64>,
    prev_dead: Vec<bool>,
}

impl PassStats {
    pub fn record(&mut self, site: &'static Location<'static>, function_scope: ScopeId) {
        self.changes += 1;
        *self.sites.entry(site).or_default() += 1;
        self.changed_fns.insert(function_scope);
    }
}

/// Nearest function scope containing `scope` (itself included), or the root scope.
pub fn enclosing_fn(scoping: &Scoping, scope: ScopeId) -> ScopeId {
    scoping
        .scope_ancestors(scope)
        .find(|&s| scoping.scope_flags(s).is_function())
        .unwrap_or_else(|| scoping.root_scope_id())
}

/// Symbols that lost a resolved reference this pass. Call before the pass is flushed.
pub fn removed_symbols(ctx: &TraverseCtx<'_>) -> Vec<SymbolId> {
    let scoping = ctx.scoping();
    let mut out = FxHashSet::default();
    for index in ctx.state.pass_changes.removed_references.ones() {
        if let Some(symbol_id) =
            scoping.get_reference(oxc_syntax::reference::ReferenceId::from_usize(index)).symbol_id()
        {
            out.insert(symbol_id);
        }
    }
    out.into_iter().collect()
}

/// AST nodes per function (excluding nested functions) and the function tree.
struct NodeCounter {
    root: ScopeId,
    stack: Vec<ScopeId>,
    order: Vec<ScopeId>,
    own: FxHashMap<ScopeId, u64>,
    parent: FxHashMap<ScopeId, ScopeId>,
    total: u64,
}

impl NodeCounter {
    fn enter_fn(&mut self, scope: Option<ScopeId>) -> bool {
        let Some(scope) = scope else { return false };
        let parent = *self.stack.last().unwrap_or(&self.root);
        self.parent.insert(scope, parent);
        self.order.push(scope);
        self.stack.push(scope);
        true
    }
}

impl<'a> Visit<'a> for NodeCounter {
    fn enter_node(&mut self, _kind: AstKind<'a>) {
        let current = *self.stack.last().unwrap_or(&self.root);
        *self.own.entry(current).or_default() += 1;
        self.total += 1;
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        let pushed = self.enter_fn(it.scope_id.get());
        walk::walk_function(self, it, flags);
        if pushed {
            self.stack.pop();
        }
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        let pushed = self.enter_fn(it.scope_id.get());
        walk::walk_arrow_function_expression(self, it);
        if pushed {
            self.stack.pop();
        }
    }
}

fn value_hashes(ctx: &TraverseCtx<'_>) -> Vec<u64> {
    let len = ctx.scoping().symbols_len();
    let mut buf = String::new();
    (0..len)
        .map(|i| {
            buf.clear();
            let _ = write!(buf, "{:?}", ctx.state.symbols.value(SymbolId::from_usize(i)));
            let mut h = FxHasher::default();
            buf.hash(&mut h);
            h.finish()
        })
        .collect()
}

/// Close `seeds` (function scopes) over their ancestor functions.
fn close_over_ancestors(
    seeds: impl IntoIterator<Item = ScopeId>,
    parent: &FxHashMap<ScopeId, ScopeId>,
    root: ScopeId,
) -> FxHashSet<ScopeId> {
    let mut out = FxHashSet::default();
    for mut s in seeds {
        while s != root && out.insert(s) {
            match parent.get(&s) {
                Some(&p) => s = p,
                None => break,
            }
        }
    }
    out.insert(root);
    out
}

/// Nodes a pass would visit when it skips the body of every function outside `dirty`
/// (the root is always visited).
fn visited_nodes(counter: &NodeCounter, subtree: &FxHashMap<ScopeId, u64>, dirty: &FxHashSet<ScopeId>) -> u64 {
    let skipped: u64 = counter
        .order
        .iter()
        .filter(|f| !dirty.contains(f) && counter.parent.get(f).is_some_and(|p| dirty.contains(p)))
        .map(|f| subtree[f])
        .sum();
    counter.total - skipped
}

#[expect(clippy::print_stderr)]
pub fn after_pass(
    program: &Program<'_>,
    ctx: &mut TraverseCtx<'_>,
    removed_syms: &[SymbolId],
    traverse: Duration,
    finish: Duration,
) {
    let Some(mut stats) = ctx.state.bg.take() else { return };
    stats.pass += 1;
    let root = ctx.scoping().root_scope_id();

    let mut counter = NodeCounter {
        root,
        stack: vec![],
        order: vec![],
        own: FxHashMap::default(),
        parent: FxHashMap::default(),
        total: 0,
    };
    counter.visit_program(program);
    let mut subtree: FxHashMap<ScopeId, u64> = FxHashMap::default();
    for f in counter.order.iter().rev() {
        let own = counter.own.get(f).copied().unwrap_or(0);
        *subtree.entry(*f).or_default() += own;
        let total = subtree[f];
        if let Some(p) = counter.parent.get(f) {
            *subtree.entry(*p).or_default() += total;
        }
    }

    // Symbols whose facts changed: lost a reference, cached value differs from last pass,
    // or newly proved dead.
    let values = value_hashes(ctx);
    let dead: Vec<bool> = (0..values.len())
        .map(|i| ctx.state.symbols.function_is_dead(SymbolId::from_usize(i)))
        .collect();
    let mut changed_syms: FxHashSet<SymbolId> = removed_syms.iter().copied().collect();
    let first = stats.prev_values.is_empty();
    for i in 0..values.len() {
        if first
            || stats.prev_values.get(i) != Some(&values[i])
            || (dead[i] && !stats.prev_dead.get(i).copied().unwrap_or(false))
        {
            changed_syms.insert(SymbolId::from_usize(i));
        }
    }
    let scoping = ctx.scoping();
    let mut fact_fns = FxHashSet::default();
    for &s in &changed_syms {
        fact_fns.insert(enclosing_fn(scoping, scoping.symbol_scope_id(s)));
        for &r in scoping.get_resolved_reference_ids(s) {
            fact_fns.insert(enclosing_fn(scoping, scoping.get_reference(r).scope_id()));
        }
    }
    let dirty_ast = close_over_ancestors(stats.changed_fns.iter().copied(), &counter.parent, root);
    let dirty_all = close_over_ancestors(
        stats.changed_fns.iter().copied().chain(fact_fns.iter().copied()),
        &counter.parent,
        root,
    );
    let visit_ast = visited_nodes(&counter, &subtree, &dirty_ast);
    let visit_all = visited_nodes(&counter, &subtree, &dirty_all);

    let mut sites: Vec<_> = stats.sites.iter().collect();
    sites.sort_by(|a, b| b.1.cmp(a.1));
    let mut top = String::new();
    for (loc, n) in sites.iter().take(6) {
        let file = loc.file().rsplit('/').next().unwrap_or(loc.file());
        let _ = write!(top, "{file}:{}x{n} ", loc.line());
    }
    #[expect(clippy::cast_precision_loss)]
    let pct = |n: u64| 100.0 * n as f64 / counter.total.max(1) as f64;
    eprintln!(
        "[oxc-bg] nodes={} pass={} trav={:.3} fin={:.3} changes={} chg_fns={} chg_syms={} fns={} \
         next_visit_ast={:.1}% next_visit_all={:.1}% top={}",
        counter.total,
        stats.pass,
        traverse.as_secs_f64() * 1000.0,
        finish.as_secs_f64() * 1000.0,
        stats.changes,
        stats.changed_fns.len(),
        changed_syms.len(),
        counter.order.len(),
        pct(visit_ast),
        pct(visit_all),
        top.trim_end(),
    );

    stats.changes = 0;
    stats.sites.clear();
    stats.changed_fns.clear();
    stats.prev_values = values;
    stats.prev_dead = dead;
    ctx.state.bg = Some(stats);
}
