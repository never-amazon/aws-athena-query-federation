use std::collections::HashMap;
use std::time::{Duration, Instant};

use athena_jdbc::sql::{JdbcConnection, SqlError};
use athena_postgresql::PgConnection;

const URL_VARIABLE: &str = "ATHENA_POSTGRESQL_IT_JDBC_URL";
const TLS_PORT_VARIABLE: &str = "ATHENA_POSTGRESQL_IT_TLS_PORT";
const TLS_PASSWORD_VARIABLE: &str = "ATHENA_POSTGRESQL_IT_TLS_PASSWORD";
const TLS_ROOT_VARIABLE: &str = "ATHENA_POSTGRESQL_IT_TLS_ROOT_CERT";
const TLS_OTHER_ROOT_VARIABLE: &str = "ATHENA_POSTGRESQL_IT_TLS_OTHER_ROOT_CERT";

fn variable(name: &str, test: &str) -> Option<String> {
    let value = std::env::var(name).ok();
    if value.is_none() {
        println!("skipping {test}: set {name} to run it");
    }
    value
}

fn rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

async fn connect(url: &str) -> Result<PgConnection, SqlError> {
    tokio::time::timeout(
        Duration::from_secs(30),
        PgConnection::connect(url, &HashMap::new()),
    )
    .await
    .expect("connect must not hang")
}

async fn scalar(connection: &PgConnection, sql: &str) -> Option<String> {
    let statement = connection.prepare_statement(sql).await.unwrap();
    let mut rows = connection.execute_query(&statement).await.unwrap();
    assert!(rows.next().await.unwrap());
    rows.current_row().unwrap().get_string(1usize).unwrap()
}

#[tokio::test]
async fn fetch_size_partial_read_of_two_million_rows_commits_without_buffering() {
    let Some(url) = variable(URL_VARIABLE, "fetch_size_partial_read") else {
        return;
    };
    let connection = connect(&url).await.unwrap();
    connection.set_auto_commit(false).await.unwrap();
    let mut statement = connection
        .prepare_statement("SELECT g, repeat('x', 100) FROM generate_series(1, 2000000) g")
        .await
        .unwrap();
    statement.set_fetch_size(1000);
    let mut rows = connection.execute_query(&statement).await.unwrap();
    assert!(rows.next().await.unwrap());
    let before = rss_kb();
    let started = Instant::now();
    connection.commit().await.unwrap();
    let took = started.elapsed();
    let grown = rss_kb().saturating_sub(before);
    println!("commit after a partial read took {took:?}, RSS grew by {grown} kB");
    assert!(took < Duration::from_secs(2), "{took:?}");
    assert!(grown < 20_000, "{grown} kB");
    drop(rows);
    assert_eq!(
        scalar(&connection, "select 42").await.as_deref(),
        Some("42")
    );
}

#[tokio::test]
async fn full_read_through_a_portal_returns_every_row() {
    let Some(url) = variable(URL_VARIABLE, "full_read_through_a_portal") else {
        return;
    };
    let connection = connect(&url).await.unwrap();
    connection.set_auto_commit(false).await.unwrap();
    let mut statement = connection
        .prepare_statement("SELECT g FROM generate_series(1, 2500) g")
        .await
        .unwrap();
    statement.set_fetch_size(1000);
    let mut rows = connection.execute_query(&statement).await.unwrap();
    let mut last = 0;
    let mut count = 0;
    while rows.next().await.unwrap() {
        last = rows
            .current_row()
            .unwrap()
            .get_int(1usize)
            .unwrap()
            .unwrap();
        count += 1;
    }
    assert_eq!((count, last), (2500, 2500));
    connection.commit().await.unwrap();
}

#[tokio::test]
async fn query_timeout_cancels_the_running_statement() {
    let Some(url) = variable(URL_VARIABLE, "query_timeout_cancels") else {
        return;
    };
    let connection = connect(&url).await.unwrap();
    let mut statement = connection
        .prepare_statement("select pg_sleep(10)")
        .await
        .unwrap();
    statement.set_query_timeout(std::time::Duration::from_secs(1));
    let started = Instant::now();
    let result = async {
        let mut rows = connection.execute_query(&statement).await?;
        rows.next().await
    }
    .await;
    let error = result.unwrap_err();
    assert_eq!(error.sql_state(), Some("57014"));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(scalar(&connection, "select 7").await.as_deref(), Some("7"));
}

#[tokio::test]
async fn dropped_reads_do_not_desynchronise_a_real_server() {
    let Some(url) = variable(URL_VARIABLE, "dropped_reads") else {
        return;
    };
    let connection = connect(&url).await.unwrap();
    let statement = connection
        .prepare_statement("select pg_sleep(1), 'slow'")
        .await
        .unwrap();
    let mut rows = connection.execute_query(&statement).await.unwrap();
    let first = tokio::time::timeout(Duration::from_millis(50), rows.next()).await;
    assert!(first.is_err());
    drop(rows);
    assert_eq!(
        scalar(&connection, "select 42").await.as_deref(),
        Some("42")
    );
}

#[tokio::test]
async fn tls_modes_follow_pgjdbc() {
    let (Some(port), Some(password), Some(root), Some(other_root)) = (
        variable(TLS_PORT_VARIABLE, "tls_modes_follow_pgjdbc"),
        variable(TLS_PASSWORD_VARIABLE, "tls_modes_follow_pgjdbc"),
        variable(TLS_ROOT_VARIABLE, "tls_modes_follow_pgjdbc"),
        variable(TLS_OTHER_ROOT_VARIABLE, "tls_modes_follow_pgjdbc"),
    ) else {
        return;
    };
    let url = |host: &str, query: &str| {
        format!(
            "jdbc:postgresql://{host}:{port}/postgres?user=postgres&password={password}&{query}"
        )
    };

    let full = connect(&url(
        "localhost",
        &format!("sslmode=verify-full&sslrootcert={root}"),
    ))
    .await
    .unwrap();
    assert_eq!(
        scalar(
            &full,
            "select ssl::text from pg_stat_ssl where pid = pg_backend_pid()"
        )
        .await
        .as_deref(),
        Some("true")
    );
    connect(&url(
        "localhost",
        &format!("sslmode=verify-full&sslrootcert={root}&channelBinding=require"),
    ))
    .await
    .unwrap();
    connect(&url(
        "localhost",
        "ssl&sslfactory=org.postgresql.ssl.NonValidatingFactory",
    ))
    .await
    .unwrap();
    connect(&url("127.0.0.1", "sslmode=require&channelBinding=require"))
        .await
        .unwrap();
    connect(&url(
        "127.0.0.1",
        &format!("sslmode=verify-ca&sslrootcert={root}"),
    ))
    .await
    .unwrap();

    for query in [
        format!("sslmode=verify-full&sslrootcert={root}"),
        "sslmode=verify-full&sslfactory=org.postgresql.ssl.NonValidatingFactory".to_string(),
    ] {
        let error = connect(&url("127.0.0.1", &query)).await.err().unwrap();
        assert_eq!(
            error.message(),
            "The hostname 127.0.0.1 could not be verified by hostnameverifier PgjdbcHostnameVerifier.",
            "{query}"
        );
        assert_eq!(error.sql_state(), Some("08006"));
    }

    let error = connect(&url(
        "localhost",
        &format!("sslmode=verify-ca&sslrootcert={other_root}"),
    ))
    .await
    .err()
    .unwrap();
    assert!(
        error.message().starts_with("SSL error: "),
        "{}",
        error.message()
    );
    assert_eq!(error.sql_state(), Some("08006"));

    let error = connect(&url(
        "localhost",
        "sslmode=prefer&password=wrong&user=postgres",
    ))
    .await
    .err()
    .unwrap();
    assert_eq!(error.sql_state(), Some("28P01"));

    default_root_cert_follows_pgjdbc(&url, &root).await;
}

async fn default_root_cert_follows_pgjdbc(url: &impl Fn(&str, &str) -> String, root: &str) {
    let home = std::env::temp_dir().join(format!("f2pg-empty-home-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    std::env::set_var("HOME", &home);
    for query in ["sslmode=verify-full", "ssl=true", "sslmode=verify-ca"] {
        let error = connect(&url("localhost", query)).await.err().unwrap();
        assert_eq!(
            error.message(),
            format!(
                "Could not open SSL root certificate file {}.",
                home.join(".postgresql/root.crt").display()
            ),
            "{query}"
        );
        assert_eq!(error.sql_state(), Some("08006"));
    }
    std::fs::create_dir_all(home.join(".postgresql")).unwrap();
    std::fs::copy(root, home.join(".postgresql/root.crt")).unwrap();
    connect(&url("localhost", "ssl")).await.unwrap();
    std::fs::remove_dir_all(&home).unwrap();
}
