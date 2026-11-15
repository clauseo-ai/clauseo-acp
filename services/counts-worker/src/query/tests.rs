//! The query guard: S23's refusal cases, scope resolution, the function allowlist, and the
//! wrapper written from the parsed statement.

use super::*;

fn refused(sql: &str) -> Refused {
    match guard(sql) {
        Ok(wrapped) => panic!("{sql} passed as {wrapped}"),
        Err(refused) => refused,
    }
}

#[test]
fn a_query_over_the_views_is_wrapped_in_the_cap() {
    let wrapped = guard("SELECT day, SUM(installs) FROM v_daily GROUP BY day").unwrap();
    assert_eq!(
        wrapped,
        "SELECT * FROM (SELECT day, SUM(installs) FROM v_daily GROUP BY day) LIMIT 1000"
    );
    for sql in [
        "select * from V_DAILY",
        "SELECT * FROM \"v_tally\" WHERE event = '_crashed'",
        "WITH c AS (SELECT * FROM v_cohort_week) SELECT * FROM c",
        "WITH a AS (SELECT 1 AS x), b AS (SELECT x FROM a) SELECT * FROM b",
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 5000) \
         SELECT i FROM n",
        "SELECT * FROM v_daily d JOIN v_monthly m ON d.version = m.version",
        "SELECT * FROM (SELECT * FROM v_paths) AS p",
        "SELECT (SELECT COUNT(*) FROM v_active) AS n",
        "SELECT * FROM v_daily WHERE version IN (SELECT version FROM v_monthly)",
        "SELECT date(day), strftime('%W', day), coalesce(agent, ''), round(avg(count), 2), \
         json_extract(cell, '$.took'), total(count), group_concat(os) FROM v_daily GROUP BY 1",
        "SELECT * FROM v_daily UNION ALL SELECT * FROM v_monthly",
        "SELECT 1;",
        "SELECT * FROM v_tally -- the crash weeks",
    ] {
        assert!(guard(sql).is_ok(), "{sql}");
    }
}

#[test]
fn a_trailing_comment_cannot_cut_the_wrapper_off() {
    let wrapped = guard("SELECT * FROM v_daily -- )").unwrap();
    assert_eq!(wrapped, "SELECT * FROM (SELECT * FROM v_daily) LIMIT 1000");
}

#[test]
fn writes_and_other_statements_are_refused() {
    for sql in [
        "INSERT INTO report VALUES (1)",
        "DELETE FROM report",
        "UPDATE rollup SET count = 0",
        "PRAGMA table_info(report)",
        "ATTACH DATABASE 'x' AS y",
        "DROP VIEW v_daily",
        "CREATE TABLE x (a)",
        "VACUUM",
    ] {
        assert!(
            matches!(refused(sql), Refused::NotQuery | Refused::Parse),
            "{sql}"
        );
    }
    assert_eq!(
        refused("WITH a AS (SELECT 1) INSERT INTO rate SELECT * FROM a"),
        Refused::NotQuery
    );
    assert_eq!(refused("SELECT 1; SELECT 2"), Refused::Statements);
    assert_eq!(
        refused("SELECT * FROM v_daily; DELETE FROM report"),
        Refused::Statements
    );
    assert_eq!(refused(""), Refused::Parse);
    assert_eq!(refused("   "), Refused::Parse);
    assert_eq!(refused("SELECT FROM WHERE"), Refused::Parse);
    assert_eq!(
        refused(&format!("SELECT '{}'", "x".repeat(MAX_SQL))),
        Refused::Parse
    );
}

#[test]
fn base_tables_are_refused_anywhere() {
    for sql in [
        "SELECT * FROM report",
        "SELECT * FROM REPORT",
        "SELECT * FROM v_daily, tally",
        "SELECT * FROM (SELECT * FROM install_week)",
        "SELECT * FROM v_daily WHERE version IN (SELECT version FROM report)",
        "SELECT (SELECT COUNT(*) FROM rollup)",
        "WITH c AS (SELECT * FROM report) SELECT * FROM c",
        "SELECT * FROM v_daily JOIN rate ON 1",
        "SELECT * FROM sqlite_master",
        "SELECT * FROM main.v_daily",
        "SELECT * FROM v_unknown",
    ] {
        assert_eq!(refused(sql), Refused::Table, "{sql}");
    }
    // SQLite's `IN table` and `TABLE name`: refused, whether the parser reads them or not.
    for sql in ["SELECT * FROM v_daily WHERE os IN report", "TABLE report"] {
        assert!(guard(sql).is_err(), "{sql}");
    }
}

#[test]
fn names_resolve_by_scope() {
    // S23's case: the CTE named report lives in the subquery only, and is refused anyway.
    assert_eq!(
        refused("SELECT * FROM (WITH report AS (SELECT 1) SELECT 1), report"),
        Refused::CteName
    );
    // A CTE of a subquery is not visible beside it.
    assert_eq!(
        refused("SELECT * FROM (WITH x AS (SELECT 1) SELECT * FROM x), x"),
        Refused::Table
    );
    assert_eq!(
        refused("SELECT (WITH x AS (SELECT 1) SELECT 1), (SELECT * FROM x)"),
        Refused::Table
    );
    // An enclosing query's CTE is visible inside.
    assert!(guard("WITH x AS (SELECT * FROM v_daily) SELECT * FROM (SELECT * FROM x)").is_ok());
    for name in [
        "report",
        "tally",
        "install_week",
        "rollup",
        "rate",
        "crash",
        "release",
        "Report",
        "sqlite_master",
        "sqlite_temp",
        "_cf_kv",
        "d1_migrations",
    ] {
        let sql = format!("WITH {name} AS (SELECT 1) SELECT * FROM {name}");
        assert_eq!(refused(&sql), Refused::CteName, "{sql}");
    }
}

#[test]
fn only_allowlisted_functions_and_plain_from_items() {
    for sql in [
        "SELECT load_extension('x')",
        "SELECT sqlite_version()",
        "SELECT readfile('/etc/passwd')",
        "SELECT * FROM v_daily WHERE random() > 0",
        "SELECT upper(os) FROM v_daily",
        "SELECT main.count(*) FROM v_daily",
    ] {
        assert_eq!(refused(sql), Refused::Function, "{sql}");
    }
    for sql in [
        "SELECT * FROM json_each('[1]')",
        "SELECT * FROM pragma_table_info('report')",
        "SELECT * FROM v_daily, json_each(v_daily.cell)",
    ] {
        assert_eq!(refused(sql), Refused::From, "{sql}");
    }
}

#[test]
fn the_view_list_is_the_migrations() {
    let migrations = [
        include_str!("../../migrations/0001_init.sql"),
        include_str!("../../migrations/0002_rollup.sql"),
        include_str!("../../migrations/0003_summaries.sql"),
        include_str!("../../migrations/0004_crash.sql"),
    ]
    .concat();
    // 0003 drops and recreates two views, so each name counts once.
    let created: std::collections::BTreeSet<&str> = migrations
        .lines()
        .filter_map(|line| line.strip_prefix("CREATE VIEW "))
        .filter_map(|rest| rest.split_whitespace().next())
        .collect();
    assert_eq!(created.len(), VIEWS.len());
    for view in VIEWS {
        assert!(created.contains(&view), "{view}");
    }
    for table in migrations
        .lines()
        .filter_map(|line| line.strip_prefix("CREATE TABLE "))
        .filter_map(|rest| rest.split_whitespace().next())
    {
        assert!(
            BASE_TABLES.contains(&table),
            "{table} is not in BASE_TABLES"
        );
    }
}

#[test]
fn the_privacy_cases_are_refused() {
    // SQLite's own tables, in any schema or spelling.
    for sql in [
        "SELECT * FROM sqlite_master",
        "SELECT * FROM sqlite_schema",
        "SELECT * FROM temp.sqlite_master",
        "SELECT * FROM main.sqlite_master",
        "SELECT * FROM \"SQLITE_MASTER\"",
        "SELECT name FROM v_daily, sqlite_temp_master",
    ] {
        assert_eq!(refused(sql), Refused::Table, "{sql}");
    }
    // A base table under every quoting and case SQLite accepts, and qualified.
    for sql in [
        "SELECT * FROM report",
        "SELECT * FROM REPORT",
        "SELECT * FROM \"report\"",
        "SELECT * FROM [report]",
        "SELECT * FROM `report`",
        "SELECT * FROM main.report",
        "SELECT * FROM \"main\".\"report\"",
        "SELECT * FROM temp.report",
        "SELECT * FROM Install_Week",
        "SELECT * FROM cron_status",
        "SELECT * FROM day_cell",
        "SELECT * FROM Active",
    ] {
        assert_eq!(refused(sql), Refused::Table, "{sql}");
    }
    // Qualified names are refused outright, a view's too: `main.v_daily` reads nothing more
    // than `v_daily`, but no schema name ever reaches SQLite, so `temp.` cannot shadow one.
    assert_eq!(refused("SELECT * FROM main.v_daily"), Refused::Table);
    // Table-valued functions, in FROM, a join or IN, and pragma functions anywhere.
    for sql in [
        "SELECT * FROM pragma_table_info('report')",
        "SELECT * FROM pragma_index_list('report')",
        "SELECT * FROM v_daily JOIN pragma_table_info('report') ON 1",
        "SELECT * FROM json_tree('{}')",
        "SELECT * FROM v_daily WHERE os IN pragma_table_info('report')",
    ] {
        assert_eq!(refused(sql), Refused::From, "{sql}");
    }
    assert_eq!(
        refused("SELECT pragma_table_info('report') FROM v_daily"),
        Refused::Function
    );
    // A base table inside a subquery or a CTE, and S23's case.
    for sql in [
        "SELECT * FROM (SELECT * FROM report)",
        "SELECT * FROM v_daily WHERE EXISTS (SELECT 1 FROM tally)",
        "WITH c AS (SELECT * FROM rollup) SELECT * FROM c",
        "WITH a AS (SELECT * FROM v_daily), b AS (SELECT * FROM [report]) SELECT * FROM a",
    ] {
        assert_eq!(refused(sql), Refused::Table, "{sql}");
    }
    assert_eq!(
        refused("SELECT * FROM (WITH report AS (SELECT 1) SELECT 1), report"),
        Refused::CteName
    );
    for sql in [
        "WITH \"report\" AS (SELECT 1) SELECT * FROM report",
        "WITH [Report] AS (SELECT 1) SELECT * FROM report",
        "WITH sqlite_master AS (SELECT 1) SELECT * FROM sqlite_master",
        "WITH cron_status AS (SELECT 1) SELECT * FROM cron_status",
    ] {
        assert_eq!(refused(sql), Refused::CteName, "{sql}");
    }
}
