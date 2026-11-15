//! `/v1/query`'s guard (decision 0061): one `SELECT` or `WITH`, parsed with `sqlparser` in
//! SQLite's dialect, that reads only the `v_*` views. Names resolve by scope: every table a
//! query names must be one of [`VIEWS`] or a CTE of an enclosing query, a CTE may not take a
//! base table's name or a `sqlite_` one, and functions come from a short allowlist. The query
//! runs as `SELECT * FROM (<query>) LIMIT 1000`, written from the parsed statement, so a
//! trailing comment cannot cut the wrapper off.

use std::ops::ControlFlow;

use sqlparser::ast::{
    Expr, InTarget, ObjectName, ObjectNamePart, Query, Select, SetExpr, Statement, TableFactor,
    Visit, Visitor,
};
use sqlparser::dialect::SQLiteDialect;
use sqlparser::parser::Parser;

/// The most rows a query returns (Workers Free: moving more out of D1 risks the 10 ms CPU).
pub(crate) const ROW_CAP: usize = 1_000;
/// The longest query text taken.
pub(crate) const MAX_SQL: usize = 8_000;

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
    Ok(format!("SELECT * FROM ({query}) LIMIT {ROW_CAP}"))
}

/// The CTE names of each enclosing query, innermost last, while the statement is walked.
#[derive(Default)]
struct Scope {
    ctes: Vec<Vec<String>>,
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
        let mut names = Vec::new();
        for cte in query.with.iter().flat_map(|with| &with.cte_tables) {
            let name = cte.alias.name.value.to_ascii_lowercase();
            if reserved(&name) || cte.from.is_some() {
                return ControlFlow::Break(Refused::CteName);
            }
            names.push(name);
        }
        self.ctes.push(names);
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, _query: &Query) -> ControlFlow<Refused> {
        self.ctes.pop();
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
        let in_scope = self.ctes.iter().any(|names| names.contains(&name));
        if in_scope || VIEWS.contains(&name.as_str()) {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(Refused::Table)
        }
    }

    fn pre_visit_table_factor(&mut self, factor: &TableFactor) -> ControlFlow<Refused> {
        match factor {
            TableFactor::Table { args: None, .. }
            | TableFactor::Derived { .. }
            | TableFactor::NestedJoin { .. } => ControlFlow::Continue(()),
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
