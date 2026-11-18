# Rust port of the Athena query federation connectors

This directory holds a Rust port of the Java federation SDK and of the JDBC, PostgreSQL and Snowflake connectors. The Rust Lambda functions accept the same Athena request JSON as the Java ones and return the same responses. Java bugs found during the port were fixed in both the Java and the Rust code, so the two sides match. [Differences from the Java connectors](#differences-from-the-java-connectors) lists what still differs.

## Crates

| Crate | Java counterpart |
|---|---|
| `athena-federation-sdk` | `athena-federation-sdk`, except the `com.amazonaws.athena.connector.substrait` package |
| `athena-federation-substrait` | `com.amazonaws.athena.connector.substrait` in `athena-federation-sdk`, plus the Calcite and Isthmus code that renders Substrait plans as SQL |
| `athena-jdbc` | `athena-jdbc` |
| `athena-postgresql` | `athena-postgresql` |
| `athena-snowflake` | `athena-snowflake` |

`athena-federation-substrait` builds the Substrait protobuf types from the protos in `athena-federation-substrait/proto` with `prost-build` and `protox`, so the build does not need `protoc`. It renders SQL for the ANSI, PostgreSQL and Snowflake dialects with its own port of the Calcite `RelToSqlConverter` subset that the Java connectors use.

`athena-postgresql` talks to PostgreSQL through its own client on the `postgres-protocol` crate, in place of pgjdbc. `athena-snowflake` uses `snowflake-connector-rs` in place of the Snowflake JDBC driver.

## Toolchain

`rust-toolchain.toml` pins Rust 1.96.0 with the `clippy` and `rustfmt` components. To install that toolchain, run this command from `rust/`:

```
rustup toolchain install
```

The workspace denies the clippy `all` and `pedantic` groups in `Cargo.toml`.

## Build, lint and test

Run these commands from `rust/`. The CI workflow `.github/workflows/rust.yml` runs the same commands.

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

`cargo test` runs offline. The tests that need a PostgreSQL server skip themselves unless `ATHENA_POSTGRESQL_IT_JDBC_URL` is set. To run them against a local container:

```
export PGPASSWORD="$(openssl rand -hex 16)"
docker run -d --name athena-rust-it -e POSTGRES_PASSWORD="$PGPASSWORD" -p 127.0.0.1:5432:5432 postgres:16
export ATHENA_POSTGRESQL_IT_JDBC_URL="jdbc:postgresql://127.0.0.1:5432/postgres?user=postgres&password=$PGPASSWORD"
cargo test -p athena-postgresql --test postgresql_integ_test --test driver_integ_test
docker rm -f athena-rust-it
```

The TLS test `tls_modes_follow_pgjdbc` in `driver_integ_test` also needs `ATHENA_POSTGRESQL_IT_TLS_PORT`, `ATHENA_POSTGRESQL_IT_TLS_PASSWORD`, `ATHENA_POSTGRESQL_IT_TLS_ROOT_CERT` and `ATHENA_POSTGRESQL_IT_TLS_OTHER_ROOT_CERT`, and skips without them. No test runs against a live Snowflake account.

## Lambda binaries

Each connector crate has two binaries in `src/bin`. They match the Java handler classes that the Java templates name.

| Binary | Java handler |
|---|---|
| `athena-postgresql` | `com.amazonaws.athena.connectors.postgresql.PostGreSqlCompositeHandler` |
| `athena-postgresql-mux` | `com.amazonaws.athena.connectors.postgresql.PostGreSqlMuxCompositeHandler` |
| `athena-snowflake` | `com.amazonaws.athena.connectors.snowflake.SnowflakeCompositeHandler` |
| `athena-snowflake-mux` | `com.amazonaws.athena.connectors.snowflake.SnowflakeMuxCompositeHandler` |

The binaries run on the `provided.al2023` Lambda runtime on x86_64. Lambda runs the file named `bootstrap` at the root of the deployment package.

### Package the binaries

To build the four binaries and put each in its own zip, run from `rust/`:

```
cargo build --release --target x86_64-unknown-linux-gnu --bins -p athena-postgresql -p athena-snowflake
for bin in athena-postgresql athena-postgresql-mux athena-snowflake athena-snowflake-mux; do
    mkdir -p target/lambda/$bin
    cp target/x86_64-unknown-linux-gnu/release/$bin target/lambda/$bin/bootstrap
    (cd target/lambda/$bin && rm -f bootstrap.zip && zip -q bootstrap.zip bootstrap)
done
```

Each zip lands in `target/lambda/<binary>/bootstrap.zip`, which is the path the templates use. `provided.al2023` has glibc 2.34, so build on a host with glibc 2.34 or older, such as Amazon Linux 2 or Amazon Linux 2023. To check a binary, list the glibc versions it needs:

```
objdump -T target/lambda/athena-postgresql-mux/bootstrap | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1
```

[cargo-lambda](https://www.cargo-lambda.info/) is optional, and the commands above do not need it. `cargo lambda build --release --output-format zip` writes the zips to the same `target/lambda/<binary>/bootstrap.zip` paths. Its documentation describes how to build on a host with a newer glibc.

### Deploy with the templates

`athena-postgresql/` and `athena-snowflake/` hold SAM templates with the same file names as the Java templates. They keep every parameter, environment variable, IAM policy statement, VPC setting and condition of the Java templates, and differ only where the runtime requires it:

- `Runtime: provided.al2023`, `Handler: bootstrap`, and a `CodeUri` that points at `../target/lambda/<binary>/bootstrap.zip`, in place of the ECR image or the Java jar.
- `athena-postgresql.yaml` and `athena-postgresql-package.yaml` select the binary with the `CompositeHandler` parameter. Java passes the handler class to the image or the jar, but a zip package has one `bootstrap`. Each of these templates has two functions, `JdbcConnectorConfig` for `PostGreSqlMuxCompositeHandler` and `JdbcConnectorConfigSingle` for `PostGreSqlCompositeHandler`. The added conditions `IsMuxHandler` and `IsSingleHandler` create one of them.
- `LicenseUrl` and `ReadmeUrl` point at the files in the Java module directories, because the Rust crate directories have no copies.

The templates read the zips from local paths, so package them first, then run `sam deploy --guided` with the template from its own directory, for example:

```
cd athena-postgresql
sam deploy --guided --template-file athena-postgresql.yaml
```

The Snowflake templates set `SNOWFLAKE_ENABLE_S3_EXPORT`, as the Java templates do. The Java and the Rust connectors both read the lowercase key `snowflake_enable_s3_export`, so the uppercase variable has no effect on either. The Rust binaries read every other environment variable that the templates set.

## Parity tooling

`parity/` holds the checks that compare the port with the Java code. They read the Java sources from the module directories at the repository root.

### Test ledger

`parity/tests.tsv` maps every Java `@Test` method in the four modules to a Rust test, with the columns `java_test`, `rust_test`, `status` and `note`. The status is `ported` or `adapted`, and an `adapted` row says in `note` how the Rust test differs. `parity/check_parity.py` checks that every Java test has a row, that each named Rust test exists, and that it passes. It runs `cargo test --workspace` itself, unless you pass `--skip-cargo`.

```
python3 parity/check_parity.py --modules athena-federation-sdk,athena-jdbc,athena-postgresql,athena-snowflake
```

### Class table

`parity/classes.tsv` maps every public top-level Java type to a Rust item, with the status `ported`, `merged` or `not_ported`. `parity/check_classes.py` checks one module at a time:

```
python3 parity/check_classes.py --module athena-snowflake
```

### Wire harness

`parity/wire/run.sh` sends the same Athena request JSON to the Java and the Rust PostgreSQL handlers and compares the responses. It starts a seeded `postgres:16` container published on `127.0.0.1` only, builds the Java helper in `parity/wire/java` with Maven in offline mode, and generates the request corpus in `parity/wire/corpus`. It needs Docker, Java, Maven (`MVN`, default `~/opt/apache-maven-3.9.9/bin/mvn`), and the Java connector jars at version 2022.47.1 in `~/.m2`. To install the jars, run `mvn install -DskipTests` on the four Java modules first.

```
parity/wire/run.sh --rust 'cargo run --release -p athena-postgresql --example parity_wire_runner'
```

- `--rust <command>` runs the Rust side after the Java side. Without it, the script runs the Java side only. `athena-postgresql/examples/parity_wire_runner.rs` is the Rust runner.
- `--zone <zone>` runs both handlers in another default time zone. The default is `UTC`.
- `--filter <text>` runs only the cases whose name contains the text.

The script writes `parity/wire/out/diff-java-rust.txt` and exits with status 1 when a case differs. `parity/wire/rules.tsv` lists the normalization rules, for example random UUIDs and Arrow buffer padding, with the reason for each.

### Calcite probes

`parity/probes/calcite/regenerate.sh` runs the Java SDK's Substrait-to-SQL code on the queries in `parity/probes/calcite/queries` and writes the expected SQL that the `athena-federation-substrait` tests compare against. It needs Java and the SDK `withdep` and `tests` jars, from `~/.m2` or from `athena-federation-sdk/target`. Without an argument it overwrites `athena-federation-substrait/tests/fixtures`. To compare without overwriting, pass another directory:

```
parity/probes/calcite/regenerate.sh /tmp/calcite-fixtures
```

## Differences from the Java connectors

Each item below is true at commit 848a84fc. The test ledger notes in `parity/tests.tsv` cover the smaller differences in test setup, such as fakes in place of Mockito mocks.

### Behavior that still differs

PostgreSQL:

- The Rust client honors `sslmode`, `ssl`, `sslfactory` (the LibPQ, NonValidating and DefaultJava factories), `sslNegotiation`, `sslrootcert`, `sslResponseTimeout`, `channelBinding`, `requireAuth`, `scramMaxIterations`, `connectTimeout`, `socketTimeout`, `loginTimeout`, `cancelSignalTimeout`, `tcpKeepAlive`, `ApplicationName`, `currentSchema`, `options`, `stringtype` and `logServerErrorDetail`. It ignores `defaultRowFetchSize`, `sslcert`, `sslkey`, `sslpassword`, `sslhostnameverifier`, `prepareThreshold`, `binaryTransfer`, `targetServerType`, `loadBalanceHosts` and `readOnly`.
- The Rust client sends no client certificate, and GSS and SSPI logins fail.
- With `sslfactory` set to the DefaultJava factory, Rust trusts the Mozilla roots from `webpki-roots`. Java trusts the JVM truststore.
- For a filter date before 4713 BC, pgjdbc sends `-infinity` and the query runs. Rust sends the real BC value, PostgreSQL rejects it as out of range, and the query fails.
- Rust reads the default time zone from `TZ` only, and falls back to UTC. Java also reads `-Duser.timezone` and `/etc/localtime`. On Lambda, `TZ` is `:UTC`, so both use UTC.

Snowflake:

- Rust reads results as JSON through `snowflake-connector-rs`. If the connection string sets `QUERY_RESULT_FORMAT=ARROW`, every Rust query fails.
- Rust does not pass the connection string values `loginTimeout` and `networkTimeout` to the driver. Login uses the driver's fixed 120 second timeout, and a query response times out after 15 minutes.
- Rust reads `CLIENT_TIMESTAMP_TYPE_MAPPING` from the connection string only, with `TIMESTAMP_LTZ` as the default. An account-level value is not applied, so filters on `TIMESTAMP_NTZ` or `TIMESTAMP_TZ` columns can shift by the session zone offset.
- Java `SnowflakeUtils` writes the JVM CA certificates to `/tmp/cacert.pem` and sets the native environment variables at startup. Rust has no counterpart (`classes.tsv` marks the class `not_ported`). The Rust HTTPS client trusts the operating system store, so a CA added only to a JVM truststore is not trusted.
- For timestamps before 1582-10-15, the Snowflake JDBC driver maps some different server values to one value, and Java returns the earliest of them. Rust returns the exact server value.
- In a JVM whose default zone is not UTC, Java takes the DATEDAY of a Snowflake timestamp column in that zone. Rust takes the UTC day. On Lambda both use UTC.

SDK and JDBC:

- `aws-esdk` has no caching materials manager. Java caches one per KMS key for 24 hours. Rust caches only the keyring, so each decrypt of a FAS token calls KMS.
- Rust stores dates and timestamps with `chrono`, which ends at year 262143. Filter values beyond that year clamp to the `chrono` limit, where Java binds the exact value.
- The `chrono-tz` tables end in 2099. In a time zone with daylight saving time, Rust uses standard time for later dates, where Java keeps the daylight saving rules.
- Error text from JSON parsing (serde_json in place of Jackson) and from Substrait plan decoding (prost in place of protobuf-java) differs. These are the only differences in the wire harness.
- A `ClassCastException` message for a class outside the JDK, such as a Calcite or SDK class, has no "is in unnamed module of loader" suffix. JDK classes keep the suffix.
- Lambda error responses carry `errorType` and `errorMessage` with the Java exception class name, and no Java stack trace.
- Rust emulates Java `HashMap` and `HashSet` iteration order. When two `TableName` keys have the same hash code, Java orders them by identity hash and Rust uses a fixed rule, so the `ListTables` order can differ. This needs 8 or more tables in one hash bucket.

### Java bugs fixed on both sides

The port found these bugs in the Java code. The Java code is fixed, and the Rust code behaves the same way. To list the commits, run `git log --oneline --no-merges 61974a93..848a84fc -- athena-federation-sdk/src/main athena-jdbc/src/main athena-postgresql/src/main athena-snowflake/src/main`. Three commits in that range are not in the list below. 77f3cb82 adds a static import, and a7af4f39 speeds up the Snowflake Julian adjustment. 42402dfd fixes Java to keep the era in Snowflake expanded DATE literals, which Rust already did.

Query generation and binds:

- Java visited scalar subqueries twice and bound wrong values, and rejected string literals above U+00FF (b04b8b36).
- Snowflake split SQL had bad spacing and two ORDER BY clauses, and dropped paging under LIMIT (89213312). On the query plan path, Java ignored Snowflake split paging (9be8bdfb) and read the PostgreSQL parent table for every partition split, which returned duplicate rows (d8e4b835).
- Java did not escape backslashes in Snowflake export literals, and its regex placeholder fill misread `$`, `\` and `?` (ab5cb20e). It did not escape quotes in Snowflake primary-key and table identifiers, the storage integration name or the export path (515d0719).
- Java rendered `CHARACTER SET "UTF-8"` in Snowflake casts, which Snowflake rejects (fc7090a1), and cut CHAR and VARCHAR cast literals by UTF-16 units (c512c6aa).
- Java truncated pre-epoch timestamp literal fractions and rendered years outside 0001 to 9999 without an error (9cb2e5a7).
- Java bound PostgreSQL DATEMILLI filters as local date-times, so timestamptz filters matched the wrong rows outside a UTC JVM (f971a0c6). It bound pre-1582 filter values on the hybrid Julian and Gregorian calendar (baa0f73f).
- An integer overflow in the Snowflake JDBC driver made Java send wrong binds and reject reads before about 3500 BC (0a759e83).

Dates and timestamps read from the database:

- Java shifted pre-1582 values: DATEMILLI values (7b206bf1), array elements (d6f847bf), passthrough export (10bbec3a), and DATE columns that Glue declares as timestamp (e2cb2bbb).
- Java read 1 BC as 1 AD and failed on BC leap days (4e31022e), and read BC timestamptz values in the JVM zone (11962ee8).
- Java read DATE values from year 10000 and later as year 0 (1e1539e2), and DATE array elements wrapped for infinity (16cf2c63).
- Java mapped PostgreSQL `timestamptz[]` to VARCHAR (5449759d).
- The DATEDAY of a timestamp kept the calendar shift for array elements (0fa7525a), for PostgreSQL timestamp columns between 1582-10-05 and 1582-10-14 (76f66e03), and for Snowflake timestamp columns, where it was up to 26 days off before 3500 BC (73c84bdf).
- Java accepted overflowing PostgreSQL date and time fields without an error (4d85587d).

SDK:

- Java accepted Glue decimal types that Java Arrow cannot read (a98fd6d4).
- Java threw `NullPointerException` on record batches with an empty body, so zero-argument UDFs failed (e1c69495).
- `BlockUtils` write errors printed `[Ljava.lang.Integer;@<hash>` in place of the array elements (a7d8219c).
