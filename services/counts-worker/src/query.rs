//! `/v1/query`'s guard (decision 0061): one `SELECT` or `WITH`, parsed with `sqlparser` in
//! SQLite's dialect, that reads only the `v_*` views. Names resolve by scope: every table a
//! query names must be one of [`VIEWS`] or a CTE of an enclosing query, a CTE may not take a
//! base table's name or a `sqlite_` one, and functions come from a short allowlist. No CTE may
//! be recursive (`WITH RECURSIVE`, or one that names itself or a later CTE, which SQLite runs
//! as recursive without the keyword): an unbounded one would keep D1 busy and block the counts
//! writes. And a query weighs at most [`MAX_TABLES`] tables, a CTE reference weighing as many
//! as its CTE reads, so a chain of CTEs cannot double a join inside the cap. That is a coarse
//! limit only: D1's own per-query time limit is the real bound on what a query costs. The query
//! runs as `SELECT * FROM (<query>) LIMIT 1000`, written from the parsed statement, so a
//! trailing comment cannot cut the wrapper off.

use std::ops::ControlFlow;

use sqlparser::ast::{
    Cte, Expr, InTarget, ObjectName, ObjectNamePart, Query, Select, SetExpr, Statement,
    TableFactor, Visit, Visitor, visit_relations,
};
use sqlparser::dialect::SQLiteDialect;
use sqlparser::parser::Parser;

/// The most rows a query returns (Workers Free: moving more out of D1 risks the 10 ms CPU).
pub(crate) const ROW_CAP: usize = 1_000;
/// The longest query text taken.
pub(crate) const MAX_SQL: usize = 8_000;
/// The most tables a query may weigh, over the whole statement: a view counts one, a reference
/// to a CTE counts what that CTE's body weighs (at least one), and a subquery in `FROM` what
/// its body weighs (at least one). A CTE that is never referenced weighs nothing.
pub(crate) const MAX_TABLES: usize = 4;

/// The views a query may read (`migrations/0002_rollup.sql`, `0003_summaries.sql` and
/// `0004_crash.sql`).
pub(crate) const VIEWS: [&str; 7] = [
    "v_active",
    "v_cohort_week",
    "v_crash",
    "v_daily",
    "v_monthly",
    "v_paths",
    "v_tally",
];

/// The base tables, whose names no CTE may take; with `sqlite_`, `_cf_` and `d1_` names.
pub(crate) const BASE_TABLES: [&str; 12] = [
    "report",
    "tally",
    "rate",
    "install_week",
    "rollup",
    "crash",
    "crash_rate",
    "release",
    "d1_migrations",
    "cron_status",
    "day_cell",
    "active",
];

/// The functions a query may call: aggregates, and a few scalars for dates and the cells.
pub(crate) const FUNCTIONS: [&str; 12] = [
    "count",
    "sum",
    "total",
    "avg",
    "min",
    "max",
    "group_concat",
    "date",
    "strftime",
    "coalesce",
    "round",
    "json_extract",
];

/// Why a query was refused; each is answered `400` with an empty body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refused {
    /// Empty, too long, or not SQL the parser reads.
    Parse,
    /// Not exactly one statement.
    Statements,
    /// A statement other than a query, or a query that writes (`WITH … INSERT`).
    NotQuery,
    /// A name that is not a view or a CTE in scope, or a qualified name.
    Table,
    /// A CTE named like a base table or SQLite's own.
    CteName,
    /// A function off the allowlist.
    Function,
    /// A table-valued function or another kind of `FROM` item.
    From,
    /// A clause SQLite does not have (`INTO`, locks, pipes, …).
    Clause,
    /// A recursive CTE.
    Recursive,
    /// More than [`MAX_TABLES`] `FROM` and `JOIN` items.
    Tables,
}

/// The query to run for `sql`: the parsed statement, written out and wrapped in the row cap.
pub(crate) fn guard(sql: &str) -> Result<String, Refused> {
    if sql.trim().is_empty() || sql.len() > MAX_SQL {
        return Err(Refused::Parse);
    }
    let statements = Parser::parse_sql(&SQLiteDialect {}, sql).or(Err(Refused::Parse))?;
    let [statement] = statements.as_slice() else {
        return Err(Refused::Statements);
    };
    let Statement::Query(query) = statement else {
        return Err(Refused::NotQuery);
    };
    let mut scope = Scope::default();
    if let ControlFlow::Break(refused) = statement.visit(&mut scope) {
        return Err(refused);
    }
    if scope.weight() > MAX_TABLES {
        return Err(Refused::Tables);
    }
    Ok(format!("SELECT * FROM ({query}) LIMIT {ROW_CAP}"))
}

/// A query body whose weight is kept apart while it is walked.
enum Body {
    /// The body of CTE `.1` of the WITH scope at depth `.0`: its weight goes to that CTE.
    Cte(usize, usize),
    /// A subquery in `FROM`: its weight goes to the query around it.
    Derived,
}

/// While the statement is walked: the CTEs of each enclosing query, innermost last, each with
/// its name and weight; the bodies whose weight is kept apart, by address; and one running
/// weight per body being walked, the statement's first.
struct Scope {
    ctes: Vec<Vec<(String, usize)>>,
    bodies: Vec<(*const Query, Body)>,
    weights: Vec<usize>,
}

impl Default for Scope {
    fn default() -> Scope {
        Scope {
            ctes: Vec::new(),
            bodies: Vec::new(),
            weights: vec![0],
        }
    }
}

impl Scope {
    /// The statement's weight, once walked.
    fn weight(&self) -> usize {
        self.weights.first().copied().unwrap_or(0)
    }

    fn add(&mut self, weight: usize) {
        if let Some(last) = self.weights.last_mut() {
            *last = last.saturating_add(weight);
        }
    }

    /// What a table named `name` weighs here: its CTE's weight, innermost scope first, or one.
    fn weight_of(&self, name: &str) -> usize {
        self.ctes
            .iter()
            .rev()
            .flat_map(|scope| scope.iter())
            .find(|(cte, _)| cte == name)
            .map_or(1, |(_, weight)| *weight)
    }

    fn body(&self, query: &Query) -> Option<usize> {
        self.bodies
            .iter()
            .position(|(body, _)| std::ptr::eq(*body, query))
    }
}

/// A single plain name, lowercased (SQLite compares names without case).
fn plain_name(name: &ObjectName) -> Option<String> {
    match name.0.as_slice() {
        [ObjectNamePart::Identifier(ident)] => Some(ident.value.to_ascii_lowercase()),
        _ => None,
    }
}

fn reserved(name: &str) -> bool {
    BASE_TABLES.contains(&name)
        || name.starts_with("sqlite_")
        || name.starts_with("_cf_")
        || name.starts_with("d1_")
}

/// Refuses a body that writes or names a table outside the walk (`TABLE name`).
fn check_body(body: &SetExpr) -> ControlFlow<Refused> {
    match body {
        SetExpr::Select(_) | SetExpr::Query(_) | SetExpr::Values(_) => ControlFlow::Continue(()),
        SetExpr::SetOperation { left, right, .. } => {
            check_body(left)?;
            check_body(right)
        }
        SetExpr::Table(_) => ControlFlow::Break(Refused::Table),
        _ => ControlFlow::Break(Refused::NotQuery),
    }
}

impl Visitor for Scope {
    type Break = Refused;

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<Refused> {
        check_body(&query.body)?;
        let foreign = !query.locks.is_empty()
            || query.for_clause.is_some()
            || query.settings.is_some()
            || query.format_clause.is_some()
            || query.fetch.is_some()
            || !query.pipe_operators.is_empty();
        if foreign {
            return ControlFlow::Break(Refused::Clause);
        }
        if query.with.as_ref().is_some_and(|with| with.recursive) {
            return ControlFlow::Break(Refused::Recursive);
        }
        let ctes: Vec<_> = query
            .with
            .iter()
            .flat_map(|with| &with.cte_tables)
            .collect();
        let mut names: Vec<String> = Vec::new();
        for cte in &ctes {
            let name = cte.alias.name.value.to_ascii_lowercase();
            if reserved(&name) || cte.from.is_some() {
                return ControlFlow::Break(Refused::CteName);
            }
            names.push(name);
        }
        // A CTE that names itself or a later one: SQLite reads it as recursive.
        for (at, cte) in ctes.iter().enumerate() {
            let later = &names[at..];
            let cycle = visit_relations(*cte, |relation| match plain_name(relation) {
                Some(name) if later.contains(&name) => ControlFlow::Break(()),
                _ => ControlFlow::Continue(()),
            });
            if cycle.is_break() {
                return ControlFlow::Break(Refused::Recursive);
            }
        }
        let depth = self.ctes.len();
        for (at, cte) in ctes.iter().enumerate() {
            let Cte { query: body, .. } = cte;
            let body: &Query = body;
            self.bodies.push((body, Body::Cte(depth, at)));
        }
        self.ctes
            .push(names.into_iter().map(|name| (name, 1)).collect());
        if self.body(query).is_some() {
            self.weights.push(0);
        }
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, query: &Query) -> ControlFlow<Refused> {
        self.ctes.pop();
        if let Some(at) = self.body(query) {
            let (_, body) = self.bodies.swap_remove(at);
            let weight = self.weights.pop().unwrap_or(0).max(1);
            match body {
                Body::Cte(depth, index) => {
                    if let Some(cte) = self
                        .ctes
                        .get_mut(depth)
                        .and_then(|ctes| ctes.get_mut(index))
                    {
                        cte.1 = weight;
                    }
                }
                Body::Derived => self.add(weight),
            }
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<Refused> {
        if select.into.is_some() {
            return ControlFlow::Break(Refused::Clause);
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_relation(&mut self, relation: &ObjectName) -> ControlFlow<Refused> {
        let Some(name) = plain_name(relation) else {
            return ControlFlow::Break(Refused::Table);
        };
        let in_scope = self
            .ctes
            .iter()
            .any(|names| names.iter().any(|(cte, _)| *cte == name));
        if in_scope || VIEWS.contains(&name.as_str()) {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(Refused::Table)
        }
    }

    fn pre_visit_table_factor(&mut self, factor: &TableFactor) -> ControlFlow<Refused> {
        match factor {
            TableFactor::Table {
                name, args: None, ..
            } => {
                let weight = plain_name(name).map_or(1, |name| self.weight_of(&name));
                self.add(weight);
                ControlFlow::Continue(())
            }
            TableFactor::Derived { subquery, .. } => {
                let body: &Query = subquery;
                self.bodies.push((body, Body::Derived));
                ControlFlow::Continue(())
            }
            TableFactor::NestedJoin { .. } => ControlFlow::Continue(()),
            _ => ControlFlow::Break(Refused::From),
        }
    }

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<Refused> {
        match expr {
            Expr::Function(function) => match plain_name(&function.name) {
                Some(name) if FUNCTIONS.contains(&name.as_str()) => ControlFlow::Continue(()),
                _ => ControlFlow::Break(Refused::Function),
            },
            Expr::In {
                target: InTarget::Table { args: Some(_), .. } | InTarget::Unnest(_),
                ..
            } => ControlFlow::Break(Refused::From),
            _ => ControlFlow::Continue(()),
        }
    }
}

#[cfg(test)]
mod tests;
