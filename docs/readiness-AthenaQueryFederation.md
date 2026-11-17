# Rewrite Readiness: AthenaQueryFederation

> Produced by `rustify-inspect-rewrite-readiness` on 2026-11-17. Read-only inspection.
> Sections marked **[scorer]** are pasted verbatim from `.rustify/readiness/readiness-sections.md`;
> do not retype, round or reorder them.

## Headline

- **Unblocked for rewrite (KPI):** no
- **Readiness score (advisory; the KPI line above is the verdict):** 77.9 / 100 (raw 77.9, band `ready-with-work`, confidence 98%, caps: none)
- **Binding blocker:** none
- **Community-built counted as Rust support:** no (default)
- **KPI category:** 2 (Request/response, read-only, other dependencies)
- **Next skill:** rustify-assess-rewrite-worthiness

## Worthiness

No worthiness assessment yet. `rustify-assess-rewrite-worthiness` is the first next skill.

## KPI checklist

Source: KPI proposal Appendix C (draft). Unblocked = every item met.

| Item | Status | Derived from |
|---|---|---|
| Integration tests in place | met | test.integration=pass |
| Production playbooks and readiness for its tier | not-met | infra.playbooks=fail |
| A standard setup path for its category | not-met | archetype=request-response, infra.hosting=partial |
| Rust support for all of its shared dependencies | TK | unverified families: io.substrait.isthmus, net.snowflake.client, org.apache.calcite |
| Local regression testing against captured production traffic | not-met | test.traffic_capture=partial |
| A shadow testing path | not-met | test.shadow=partial |
| A cut-over and rollback plan | not-met | infra.cutover=partial |

## Service profile

| Fact | Value | Evidence |
|---|---|---|
| Archetype (per entry point) | request-response | `MetadataHandler`, `RecordHandler`, `UserDefinedFunctionHandler` and `CompositeHandler` are Lambda `RequestStreamHandler` classes. Athena invokes them synchronously and waits for one JSON response. The deployed entry points are `PostGreSqlMuxCompositeHandler` or `PostGreSqlCompositeHandler` and `SnowflakeMuxCompositeHandler`. |
| Operations | 9 (scan 0, model none; reconciled against the SDK request types) | `FederationRequest.java:41-50` lists ListSchemas, ListTables, GetTable, GetTableLayout, GetSplits, ReadRecords, UserDefinedFunction, Ping and GetDataSourceCapabilities. The scan finds 0 because there are no route annotations. |
| Mutating operations | 0 | No operation writes to the data source. ReadRecords writes spill objects to S3, but those objects are temporary result transport that Athena reads and discards, not service state. If you count spill writes as mutations, the KPI category moves from 2 to 3. |
| Internal (non-AWS) downstreams | 2 | The PostgreSQL database (over the PostgreSQL JDBC driver) and Snowflake (over the Snowflake JDBC driver). Both are customer-owned sources outside AWS service APIs. |
| Framework | other | `scan.json` finds no Coral, JAX-RS, Spring or servlet import. The framework is the SDK's own Jackson-based JSON dispatch on top of `aws-lambda-java-core`. |
| Hosting | Lambda (container image) | `AWS::Serverless::Function` with `PackageType: Image` in `athena-postgresql/athena-postgresql.yaml:76-91` and `athena-snowflake/athena-snowflake.yaml:77-92`, published through the Serverless Application Repository. |

## Score breakdown

| Dimension | Weight | Score |
|---|---|---|
| (a) Library dependencies | 30 | 27.91 |
| (b) Service infrastructure | 18 | 12.00 |
| (c) Codebase | 12 | 8.00 |
| (d) Test readiness | 25 | 15.00 |
| (e) Local-loop readiness | 15 | 15.00 |
| **Total (raw)** | 100 | 77.91 |
| **Final (after caps)** | | 77.90 |

### Library families

| Family | Map status | Gap rows | Class | Credit | Earned | Evidence |
|---|---|---|---|---|---|---|
| async-concurrency | eliminated-by-architecture | - | ready | 1.0 | 0.70 | dependency-map.md: async-concurrency -> eliminated-by-architecture (Async runtime slot, tokio) |
| aws-sdk-core | mapped-wiki | - | ready | 1.0 | 0.70 | dependency-map.md: aws-sdk-core -> aws-config (wiki row "AWSJavaClientRuntime") |
| aws-sdk:athena | external-importable | - | ready | 1.0 | 0.70 | dependency-map.md: aws-sdk:athena -> aws-sdk-athena 1.131.0 (crates.io, Apache-2.0) |
| aws-sdk:glue | external-importable | - | ready | 1.0 | 0.70 | dependency-map.md: aws-sdk:glue -> aws-sdk-glue 1.187.0 (crates.io, Apache-2.0) |
| aws-sdk:kms | external-importable | - | ready | 1.0 | 0.70 | dependency-map.md: aws-sdk:kms -> aws-sdk-kms 1.132.0 (crates.io, Apache-2.0) |
| aws-sdk:lambda | external-importable | - | ready | 1.0 | 0.70 | dependency-map.md: aws-sdk:lambda -> lambda_runtime 1.5.0 + aws-sdk-lambda 1.160.0 (crates.io, Apache-2.0) |
| aws-sdk:s3 | mapped-wiki | - | ready | 1.0 | 0.70 | dependency-map.md: aws-sdk:s3 -> aws-sdk-s3 (wiki row "AWSS3JavaClient") |
| aws-sdk:secretsmanager | external-importable | - | ready | 1.0 | 0.70 | dependency-map.md: aws-sdk:secretsmanager -> aws-sdk-secretsmanager 1.128.0 (crates.io, Apache-2.0) |
| aws-sdk:sts | external-importable | - | ready | 1.0 | 0.70 | dependency-map.md: aws-sdk:sts -> aws-sdk-sts 1.126.0 (crates.io, Apache-2.0) |
| cache-in-process | mapped-wiki | - | ready | 1.0 | 0.70 | dependency-map.md: cache-in-process -> moka (wiki row "Caffeine / Google Guava Cache") |
| com.amazonaws.encryptionsdk | external-importable | - | ready | 1.0 | 0.70 | dependency-map.md: com.amazonaws.encryptionsdk -> aws-esdk 1.3.0 (crates.io, ISC AND (Apache-2.0 OR ISC)) |
| com.sun.jna | eliminated-by-architecture | - | ready | 1.0 | 0.70 | dependency-map.md: com.sun.jna -> eliminated-by-architecture (LibC.setenv for SSL_CERT_FILE; std::env or rustls roots) |
| com.zaxxer.hikari | external-importable | - | ready | 1.0 | 0.70 | dependency-map.md: com.zaxxer.hikari -> deadpool-postgres 0.14.2 or the sqlx pool (crates.io) |
| datetime | mapped-wiki | - | ready | 1.0 | 0.70 | dependency-map.md: datetime -> chrono (wiki row "Joda-Time / JSR-310") |
| edu.umd.cs | eliminated-by-architecture | - | ready | 1.0 | 0.70 | dependency-map.md: edu.umd.cs -> eliminated-by-architecture (SpotBugs annotations, build-time only) |
| encoding-hashing | mapped-wiki | - | ready | 1.0 | 0.70 | dependency-map.md: encoding-hashing -> base64 + hex + sha2 (wiki row "Apache Commons Codec") |
| error-handling | mapped-wiki | - | ready | 1.0 | 0.70 | dependency-map.md: error-handling -> thiserror + anyhow (wiki row "Java Exceptions") |
| http-client | mapped-wiki | - | ready | 1.0 | 0.70 | dependency-map.md: http-client -> reqwest (wiki row "Apache HttpClient / Jersey Client") |
| io.substrait.expression | external-importable | - | ready | 1.0 | 0.70 | dependency-map.md: io.substrait.expression -> substrait 0.65.0 (crates.io, Apache-2.0) |
| io.substrait.extension | external-importable | - | ready | 1.0 | 0.70 | dependency-map.md: io.substrait.extension -> substrait 0.65.0 (crates.io, Apache-2.0) |
| io.substrait.isthmus | unverified | - | unknown | 0.0 | 0.00 | dependency-map.md: io.substrait.isthmus -> unverified: no Rust Isthmus; candidate datafusion-substrait 55.2.0 + DataFusion SQL unparser, parity with Isthmus SQL output not checked |
| io.substrait.plan | external-importable | - | ready | 1.0 | 0.70 | dependency-map.md: io.substrait.plan -> substrait 0.65.0 (crates.io, Apache-2.0) |
| io.substrait.proto | external-importable | - | ready | 1.0 | 0.70 | dependency-map.md: io.substrait.proto -> substrait 0.65.0 + prost 0.14.6 (crates.io, Apache-2.0) |
| io.substrait.util | external-importable | - | ready | 1.0 | 0.70 | dependency-map.md: io.substrait.util -> substrait 0.65.0 (crates.io, Apache-2.0) |
| java.beans | eliminated-by-architecture | - | ready | 1.0 | 0.70 | dependency-map.md: java.beans -> eliminated-by-architecture (@Transient serialization hint; serde attributes) |
| java.util.AbstractMap | eliminated-by-architecture | - | ready | 1.0 | 0.70 | dependency-map.md: java.util.AbstractMap -> eliminated-by-architecture (JDK collection; Rust std) |
| java.util.Map | eliminated-by-architecture | - | ready | 1.0 | 0.70 | dependency-map.md: java.util.Map -> eliminated-by-architecture (JDK collection; Rust std) |
| javax.annotation | eliminated-by-architecture | - | ready | 1.0 | 0.70 | dependency-map.md: javax.annotation -> eliminated-by-architecture (nullability annotations) |
| javax.crypto | external-importable | - | ready | 1.0 | 0.70 | dependency-map.md: javax.crypto -> aes-gcm 0.11.1 (crates.io, Apache-2.0 OR MIT) |
| javax.crypto.spec | external-importable | - | ready | 1.0 | 0.70 | dependency-map.md: javax.crypto.spec -> aes-gcm 0.11.1 (crates.io, Apache-2.0 OR MIT) |
| logging | mapped-wiki | - | ready | 1.0 | 0.70 | dependency-map.md: logging -> tracing + tracing-subscriber (wiki row "Slf4j / Log4j") |
| net.snowflake.client | unverified | - | unknown | 0.0 | 0.00 | dependency-map.md: net.snowflake.client -> unverified: no first-party Rust driver; community snowflake-connector-rs 1.1.1 (MIT) or the Snowflake SQL REST API over reqwest, feature parity with snowflake-jdbc not checked |
| org.apache.arrow | external-importable | - | ready | 1.0 | 0.70 | dependency-map.md: org.apache.arrow -> arrow 60.1.0 + arrow-ipc + parquet 60.1.0 (crates.io, Apache-2.0) |
| org.apache.calcite | unverified | - | unknown | 0.0 | 0.00 | dependency-map.md: org.apache.calcite -> unverified: no Rust Calcite; candidates sqlparser 0.64.0 or the DataFusion unparser, dialect SQL parity not checked |
| org.postgresql.core | external-importable | - | ready | 1.0 | 0.70 | dependency-map.md: org.postgresql.core -> tokio-postgres 0.7.18 or sqlx 0.9.0 (crates.io, MIT OR Apache-2.0) |
| serialization-json | mapped-wiki | - | ready | 1.0 | 0.70 | dependency-map.md: serialization-json -> serde + serde_json (wiki row "Jackson (core, databind, annotations)") |
| serialization-protobuf | external-importable | - | ready | 1.0 | 0.70 | dependency-map.md: serialization-protobuf -> prost 0.14.6 (crates.io, Apache-2.0) |
| software.amazon.awssdk | mapped-wiki | - | ready | 1.0 | 0.70 | dependency-map.md: software.amazon.awssdk -> aws-config (wiki row "AWSJavaClientRuntime") |
| storage-sql | external-importable | - | ready | 1.0 | 0.70 | dependency-map.md: storage-sql -> sqlx 0.9.0 or per-driver crates (crates.io, MIT OR Apache-2.0) |
| test-harness | mapped-wiki | - | ready | 1.0 | 0.70 | dependency-map.md: test-harness -> #[test] + tokio::test + mockall (wiki rows "JUnit / TestNG", "Mockito"), test-only |
| text-regex | external-importable | - | ready | 1.0 | 0.70 | dependency-map.md: text-regex -> regex 1.13.1 (crates.io, MIT OR Apache-2.0) |
| tls | external-importable | - | ready | 1.0 | 0.70 | dependency-map.md: tls -> rustls 0.23.46 + rustls-pemfile 2.2.0 + pkcs8 0.11.0 + rsa 0.10.0 (crates.io) |
| utilities-collections | mapped-wiki | - | ready | 1.0 | 0.70 | dependency-map.md: utilities-collections -> std + itertools (wiki rows "Google Guava", "Apache Commons Lang/IO") |

### Checks

| Check | Points | Status | Earned | Evidence or TK |
|---|---|---|---|---|
| `infra.hosting` Hosting is on the rustify-scaffold-cdk-infra route for its archetype | 3 | partial | 1.50 | lambda for request-response (rustify-scaffold-cdk-infra routing): AWS::Serverless::Function with PackageType Image in athena-postgresql/athena-postgresql.yaml:76-91 and athena-snowflake/athena-snowflake.yaml:77-92, published through the Serverless Application Repository (AWS::ServerlessRepo::Application metadata, athena-postgresql.yaml:2-14). No CDK app or Apollo environment for the connectors. |
| `infra.pipeline` Pipeline is defined as code | 2 | partial | 1.00 | GitHub Actions workflows are code (.github/workflows/maven_push.yml, cut_release.yml, publish_to_maven_central.yml, run_release_tests.yml), but they build, test and publish artifacts and SAR templates. There is no staged Alpha through Prod deployment pipeline; customers deploy the SAR application themselves. |
| `infra.auth` Inbound and outbound auth have a Rust path | 3 | pass | 3.00 | IAM only. Athena invokes the Lambda with IAM (lambda:InvokeFunction), and every outbound AWS call (S3, KMS, Glue, Athena, Secrets Manager, STS) uses the AWS SDK v2 with SigV4. Database credentials come from Secrets Manager (SecretNamePrefix policy, athena-postgresql/athena-postgresql.yaml:25-28,119). No CloudAuth, AAA, Midway or TA token in the closure. |
| `infra.discovery_mesh` Service discovery and mesh have a Rust path | 2 | pass | 2.00 | No mesh dependency. No Disco, BigMesh or Tardigrade package in the 205-row Maven closure (.rustify/map-dependencies/raw/all.txt); the Lambda reaches the database over the VPC subnets and security groups in athena-postgresql/athena-postgresql.yaml:51-54. |
| `infra.config` Configuration system has a Rust path | 2 | pass | 2.00 | Environment variables only. Handlers receive System.getenv() as configOptions (athena-jdbc/src/main/java/com/amazonaws/athena/connectors/jdbc/MultiplexingJdbcCompositeHandler.java:46-50), set from SAR template parameters; secrets come from Secrets Manager. No G2S2, AppConfig or other dynamic config system. |
| `infra.datastores` Datastores have Rust clients | 2 | partial | 1.00 | The service's own storage is AWS only: S3 for spill (S3BlockSpiller), KMS for spill keys, Glue for optional metadata. The data sources are customer databases: PostgreSQL has mature Rust clients (tokio-postgres 0.7.18, sqlx 0.9.0), but Snowflake has only the community-built snowflake-connector-rs 1.1.1 (MIT, estie-inc) or the Snowflake SQL REST API, so the Snowflake path is partial. |
| `infra.observability` Metrics can be reproduced in Rust | 1 | pass | 1.00 | No metrics are emitted. The four main trees contain no CloudWatch, EMF or metrics client calls (grep of .rustify/map-dependencies/src-union); observability is Log4j2 logging through aws-lambda-java-log4j2 to CloudWatch Logs, which tracing reproduces. |
| `infra.playbooks` Production playbooks meet the tier's requirements | 2 | fail | 0.00 | No runbook or playbook exists for these connectors. README.md:27 links only a GitHub wiki FAQ, and the module READMEs (athena-postgresql/README.md, athena-snowflake/README.md) cover deployment parameters, not operations. The open-source connectors have no owning on-call tier; each customer operates its own deployment. |
| `infra.cutover` Every caller group has a cut-over strategy with a rollback path | 1 | partial | 0.50 | Each Athena data catalog names the Lambda by ARN, so a caller moves by pointing its catalog at a new function, and rolls back by redeploying the previous SAR SemanticVersion (athena-postgresql/athena-postgresql.yaml:13). That is a per-caller endpoint move, but there is no documented plan and the templates define no Lambda alias or AutoPublishAlias with weighted routing (grep for Alias in the two templates returned nothing), so there is no server-side dial point. |
| `code.archetype` Service archetype has a Rustify golden path | 3 | pass | 3.00 | Every entry point is a Lambda RequestStreamHandler that Athena invokes synchronously and waits on: MetadataHandler.handleRequest (athena-federation-sdk/src/main/java/com/amazonaws/athena/connector/lambda/handlers/MetadataHandler.java:257), RecordHandler.handleRequest (handlers/RecordHandler.java:131), UserDefinedFunctionHandler.handleRequest (handlers/UserDefinedFunctionHandler.java:116), and CompositeHandler (handlers/CompositeHandler.java:56), which muxes the two. Deployed entry points: PostGreSqlMuxCompositeHandler or PostGreSqlCompositeHandler (athena-postgresql/athena-postgresql.yaml:55-59,91) and SnowflakeMuxCompositeHandler (athena-snowflake/athena-snowflake.yaml:92). ReadRecords returns inline Arrow blocks or S3 spill locations in one response; no stream or socket outlives the call. |
| `code.operation_count` Operation count | 2 | pass | 2.00 | No Coral or Smithy model. The scan counts 0 route annotations because the operations are JSON request types, not annotated routes. The nine operations are the @JsonSubTypes of FederationRequest (athena-federation-sdk/src/main/java/com/amazonaws/athena/connector/lambda/request/FederationRequest.java:41-50): ListSchemasRequest, ListTablesRequest, GetTableRequest, GetTableLayoutRequest, GetSplitsRequest, ReadRecordsRequest, UserDefinedFunctionRequest, PingRequest, GetDataSourceCapabilitiesRequest. Each has a request class under connector/lambda/metadata, records, request and udf. |
| `code.framework` Service framework | 2 | fail | 0.00 | scan: framework other (no framework import found) |
| `code.reflection` Reflection, dynamic proxy and runtime annotation hotspots | 2 | partial | 1.00 | scan: 9 reflection site(s); see scan.json reflection.sites |
| `code.concurrency` Shared mutable state | 2 | partial | 1.00 | scan: 25 shared-state site(s); see scan.json shared_state.sites |
| `code.native` Native code and JNI | 1 | pass | 1.00 | scan: 0 native site(s); see scan.json native.sites |
| `test.unit` Unit tests and coverage | 4 | partial | 2.00 | scan: 146 test file(s), 1176 test method(s); coverage not measured |
| `test.integration` Integration tests against a deployed stage | 5 | pass | 5.00 | PostGreSqlIntegTest (athena-postgresql/src/test/java/com/amazonaws/athena/connectors/postgresql/integ/PostGreSqlIntegTest.java:72) and SnowflakeIntegTest extend IntegrationTestBase from athena-federation-integ-test, which deploys the connector Lambda and the database through CloudFormation and queries it through Athena (athena-federation-integ-test/README.md). The daily GitHub Actions job .github/workflows/run_release_tests.yml runs release tests against deployed connectors, with SNOWFLAKE_CREDENTIAL_SECRET_NAME and an RDS secret for PostgreSQL (athena_federation_testing/resources/infra/cfn_template/postgresql.yaml). |
| `test.traffic_capture` Production traffic can be captured or replayed (equivalence oracle) | 8 | partial | 4.00 | No production capture exists: Athena calls customer-owned Lambdas, and the handlers log only toString() of the request and response (MetadataHandler.java:290-317), not the wire JSON. Inputs are synthesizable from the SDK request types, and 25 golden serde fixtures with request and response pairs exist under athena-federation-sdk/src/test/resources/serde/ (v2, v4, v5, v6, Ping). These are synthetic pairs, not production traffic, so the check is partial. |
| `test.shadow` A shadow environment or mirror point exists | 5 | partial | 2.50 | Non-production stacks exist: the integ-test framework and athena_federation_testing deploy a connector, a database and test data in a test account (validation_testing/README.md, run_release_tests.yml). Nothing can mirror production Athena traffic to a second Lambda; a shadow would need a second catalog and a replay of a fixed query set. |
| `test.determinism` Output is deterministic and diffable | 3 | partial | 1.50 | Outputs are diffable after canonicalization. GetSplits generates random split ids (UUID.randomUUID, MetadataHandler.java:253), spill locations and AES-GCM keys are random (S3BlockSpiller.java:488, LocalKeyFactory.java:51 SecureRandom), and JDBC row order is not fixed without ORDER BY. The Arrow payloads themselves are deterministic for a fixed input. |
| `loop.reference_builds` The Java reference builds in a local workspace | 3 | pass | 3.00 | All four modules build and pass their tests locally with ~/opt/apache-maven-3.9.9/bin/mvn in about 4 minutes (operator baseline: SDK 629 tests, JDBC 316, PostgreSQL 95, Snowflake 220 with 2 skipped, no failures). This run also resolved all four dependency trees offline (mvn -o dependency:tree, logs in .rustify/map-dependencies/raw/mvn-log-*.txt). |
| `loop.reference_runnable` The Java reference can answer replays | 4 | pass | 4.00 | The handlers are plain RequestStreamHandler classes (handleRequest(InputStream, OutputStream, Context), RecordHandler.java:131), so a replay harness can call them in-process with a JSON payload. PostgreSQL can run locally in Docker 25. Snowflake has no local emulator, so Snowflake replays need a non-production Snowflake account, which the release tests already use (SNOWFLAKE_CREDENTIAL_SECRET_NAME in run_release_tests.yml). |
| `loop.fast_tests` Unit tests run locally in under 10 minutes | 2 | pass | 2.00 | About 4 minutes for build plus tests of all four modules (operator-timed baseline). Not re-timed in this read-only run because a Maven test run writes target/ under the Java modules. |
| `loop.deps_stubbable` Downstream dependencies can be stubbed offline | 3 | pass | 3.00 | Clients are injected through constructors: PostGreSqlRecordHandler(DatabaseConnectionConfig, S3Client, SecretsManagerClient, AthenaClient, JdbcConnectionFactory, ...) (athena-postgresql/.../PostGreSqlRecordHandler.java:86) and SnowflakeRecordHandler (athena-snowflake/.../SnowflakeRecordHandler.java:132). The default constructors call S3Client.create(), but every test path passes Mockito mocks, which is how the 1176 test methods run offline. |
| `loop.rust_toolchain` Rust gates run locally | 3 | pass | 3.00 | Adapted for a non-Brazil project: cargo 1.96.0 and rustc 1.96.0 resolve locally, with clippy, rustfmt and cargo-zigbuild in ~/.cargo/bin. `cargo brazil` does not exist here ("no such command: brazil") and is not needed because the project builds with plain Cargo against crates.io. cargo-lambda is not installed; Lambda packaging would use the container-image route the Java templates already use. |

## Judgement notes

### Library dependencies

The closure came from Maven, not Brazil. `mvn dependency:tree` ran once per module, and a converter turned the four trees into the brazil-path file shapes that `map_dependencies.py` reads. The four in-scope modules are first-party, so the converter removed them from the tree and kept their children. The pipeline reconciled 205 closure rows: 87 family members and 118 residual rows.

No CommonLibraryGaps row matches this closure. Every gap row names an Amazon-internal library, and this service uses only open-source and AWS SDK artifacts.

Three families are `unverified` and score as TK:

- `net.snowflake.client`: Snowflake ships no first-party Rust driver. `snowflake-connector-rs` 1.1.1 (MIT, estie-inc, released 2026-10-22) is the most current community crate. `snowflake-api` 0.14.0 (mycelial) has had no release since 2025-10-23. The third option is the Snowflake SQL REST API over `reqwest`, with key-pair JWT auth through `jsonwebtoken` and `rsa`. Neither the Rustify gaps page nor its Resolved page has a Snowflake row. Nobody has checked whether any of these options covers the features the connector uses: key-pair and OAuth auth (`SnowflakeAuthUtils` in athena-snowflake, `OAuthCredentialsProvider` in the SDK), Arrow result chunks, and the S3 export path.
- `org.apache.calcite`: the JDBC query builders use Calcite `rel2sql` and the PostgreSQL and Snowflake dialects to turn pushed-down plans into SQL. Rust has no Calcite. `sqlparser` 0.64.0 and the DataFusion unparser are candidates, but nobody has checked whether they produce the same dialect SQL.
- `io.substrait.isthmus`: `SubstraitSqlUtils` uses Isthmus to convert Substrait plans to Calcite. The `substrait` 0.65.0 crate covers the plan model. `datafusion-substrait` 55.2.0 is the nearest conversion layer, and it targets DataFusion, not Calcite.

Twenty-one families are `external-importable` under an adaptation. There is no `cargo brazil`, so this run counts a crate as importable when the crates.io API returns a non-yanked version with an OSI license. The metadata is saved in `.rustify/map-dependencies/crates/`.

The scanner reports 0 native sites, but the code has two native paths that the scan rules do not detect. `SnowflakeUtils.java:87` calls `LibC.INSTANCE.setenv` through JNA to set `SSL_CERT_FILE`. `SnowflakeRecordHandler` uses the Arrow Dataset JNI (`NativeMemoryPool`, `FileSystemDatasetFactory`) to read Parquet on the S3 export path. Neither is a blocker: Rust replaces the first with `std::env` or rustls roots and the second with the `parquet` crate. They are recorded here because the score does not reflect them.

### Service infrastructure

- `infra.hosting` is partial because the scorer's route table puts request-response services on EC2 or Fargate and gives Lambda half credit. For this service, Lambda is the contract Athena calls, so a Rust port stays on Lambda. The partial status reflects the route table, not a missing path.
- `infra.pipeline` is partial. The GitHub Actions workflows are code, but they build and publish artifacts and SAR templates. No stage-by-stage deployment exists, because customers deploy the connectors into their own accounts.
- `infra.datastores` is partial because of Snowflake. The service's own state is S3 spill, KMS keys and Glue metadata, all AWS. PostgreSQL has mature crates.
- `infra.playbooks` fails. The repository has no runbook, and the README links only a GitHub wiki FAQ. As an open-source connector it has no owning on-call tier, so this KPI item may need a different reading for this project. The status records what exists.
- `infra.cutover` is partial. Each Athena catalog points at a Lambda ARN, so customers can move a catalog to a Rust function and roll back by redeploying the previous SAR version. The templates define no Lambda alias with weighted routing, and no cutover plan is written down.

### Test readiness

- `test.integration` passes. `PostGreSqlIntegTest` and `SnowflakeIntegTest` deploy the connector and a database through CloudFormation and query them through Athena. `run_release_tests.yml` runs release tests every day against deployed connectors.
- `test.traffic_capture` is partial, not fail. No production capture is possible from this repository: customers run the Lambdas, and the handlers log `toString()` output, not wire JSON. The 25 golden serde fixtures in `athena-federation-sdk/src/test/resources/serde/` are request and response pairs for every operation across serde v2, v4, v5 and v6, plus Ping. Together with the SDK request types, they make inputs synthesizable with deterministic output.
- `test.shadow` is partial. The release-test infrastructure gives a non-production connector and database, but nothing mirrors Athena traffic. A shadow means a second catalog pointed at the Rust Lambda and a replay of the release-test queries.
- `test.determinism` is partial. Split ids (`UUID.randomUUID`), spill keys and AES-GCM keys are random, and JDBC row order is not fixed. Comparisons need these fields canonicalized, and spilled blocks need decryption before comparison.

### Local-loop readiness

All five checks pass. Two rest on adaptations:

- `loop.fast_tests` uses the operator's 4-minute baseline. This run did not re-time it, because `mvn test` writes `target/` under the Java modules, which is outside this inspection's write scope.
- `loop.rust_toolchain` passes on local `cargo` 1.96.0 and `rustc` 1.96.0 instead of `cargo brazil`, which does not exist here and is not needed for a crates.io build. `cargo-lambda` is not installed. The Java connectors already ship as Lambda container images, and a Rust image can use the same route.

`loop.reference_runnable` passes for the SDK and PostgreSQL. PostgreSQL runs locally in Docker. Snowflake replays need a non-production Snowflake account, which the release tests already use.

## Blockers

None.

## Golden path

| # | Phase | Action | Skill | Why | Traces to |
|---|---|---|---|---|---|
| 1 | libraries | Resolve io.substrait.isthmus (unverified): run rustify-map-dependencies in mode: full for this family. | `rustify-map-dependencies` | unknown family earns no credit | family:io.substrait.isthmus |
| 2 | libraries | Resolve net.snowflake.client (unverified): run rustify-map-dependencies in mode: full for this family. | `rustify-map-dependencies` | unknown family earns no credit | family:net.snowflake.client |
| 3 | libraries | Resolve org.apache.calcite (unverified): run rustify-map-dependencies in mode: full for this family. | `rustify-map-dependencies` | unknown family earns no credit | family:org.apache.calcite |
| 4 | infrastructure | Pick the Rust hosting arm and scaffold its infrastructure | `rustify-scaffold-cdk-infra` | infra.hosting is partial (3 points) | infra.hosting |
| 5 | infrastructure | Bootstrap a CDK pipeline for the Rust service before writing service code | `rustify-scaffold-cdk-infra` | infra.pipeline is partial (2 points) | infra.pipeline |
| 6 | infrastructure | Confirm a Rust client for every datastore, or plan the data-access change | - | infra.datastores is partial (2 points) | infra.datastores |
| 7 | infrastructure | Write or refresh the production playbook for the service's tier before cutover planning | - | infra.playbooks is fail (2 points) | infra.playbooks |
| 8 | infrastructure | Choose the cutover strategy per caller group and confirm each has a fast rollback path | `rustify-launch` | infra.cutover is partial (1 points) | infra.cutover |
| 9 | codebase | Model the REST surface in Smithy and confirm wire compatibility for existing callers | `rustify-generate-smithy-model` | code.framework is fail (2 points) | code.framework |
| 10 | codebase | Read each reflection site and record the static Rust design that replaces it in the spec | `rustify-deep-api-research` | code.reflection is partial (2 points) | code.reflection |
| 11 | codebase | Map each shared-state site to an ownership design (Arc, channels) in the plan | `rustify-plan-migration` | code.concurrency is partial (2 points) | code.concurrency |
| 12 | test-harness | Build request/response oracle pairs with rustify-generate-test-harness (replay-logs if request logs exist, otherwise synthesize from the model) and replay them against the Java reference. | `rustify-generate-test-harness` | test.traffic_capture is partial (8 points) | test.traffic_capture |
| 13 | test-harness | Choose the shadow mechanism now (mirror point or dual subscription); it is deployed after validation | `rustify-launch` | test.shadow is partial (5 points) | test.shadow |
| 14 | test-harness | Raise unit coverage on the operations to be ported first | - | test.unit is partial (4 points) | test.unit |
| 15 | test-harness | List the canonicalization rules the comparison needs | `rustify-validate-equivalence` | test.determinism is partial (3 points) | test.determinism |
| 16 | next-skills | Decide whether the rewrite is worth it | `rustify-assess-rewrite-worthiness` | Rustify playbook order | facts.worthiness_report |
| 17 | next-skills | Fleet, traffic and performance baseline | `rustify-analyze-java-service` | Rustify playbook order | code.operation_count |
| 18 | next-skills | Full-mode pass (mode: full) for unverified and community-built families | `rustify-map-dependencies` | Rustify playbook order | dimension:library |
| 19 | next-skills | Author the MD-first plan; carry the blockers into Step 3 | `rustify-plan-migration` | Rustify playbook order | playbook:plan |
| 20 | next-skills | Capture the oracle before implementation | `rustify-generate-test-harness` | Rustify playbook order | test.traffic_capture |
| 21 | next-skills | Generate the Rust workspace | `rustify-scaffold-rust-service` | Rustify playbook order | playbook:scaffold |
| 22 | next-skills | Bootstrap the pipeline and hosting | `rustify-scaffold-cdk-infra` | Rustify playbook order | infra.hosting |
| 23 | next-skills | Behavioral spec per operation, highest traffic first | `rustify-deep-api-research` | Rustify playbook order | playbook:spec |

## Golden path notes

- Rank 9 tells you to model a REST surface in Smithy, and that step does not fit this service. The connectors have no REST surface. Their wire contract is the SDK's JSON payload with base64 Arrow IPC blocks, and Athena is the only caller. Replace this step with a serde type set per serde version (v2 to v6), checked against the golden fixtures. The step fires because `code.framework` is `other`, not Coral.
- Ranks 1 to 3 come first, and the Snowflake spike (rank 2) should run before you plan the athena-snowflake port. If the community crate falls short, the REST API route changes the auth and result-decoding design.
- Rank 12 does not need production capture. The golden serde fixtures and the release-test queries replayed against the Java Lambda can seed the oracle. Rank 15 (canonicalization rules) has to come before the oracle can compare spilled or split-bearing responses.
- Rank 4 (hosting) and rank 22 should keep Lambda. Athena only calls Lambda functions.
- Port `athena-federation-sdk` before `athena-jdbc`, and `athena-jdbc` before the two connectors. Both connectors subclass the JDBC handlers, and the JDBC handlers subclass the SDK handlers.

## Unknowns (TK)

- `family:io.substrait.isthmus`: TK. map-dependencies status unverified; dependency-map.md: io.substrait.isthmus -> unverified: no Rust Isthmus; candidate datafusion-substrait 55.2.0 + DataFusion SQL unparser, parity with Isthmus SQL output not checked
- `family:net.snowflake.client`: TK. map-dependencies status unverified; dependency-map.md: net.snowflake.client -> unverified: no first-party Rust driver; community snowflake-connector-rs 1.1.1 (MIT) or the Snowflake SQL REST API over reqwest, feature parity with snowflake-jdbc not checked
- `family:org.apache.calcite`: TK. map-dependencies status unverified; dependency-map.md: org.apache.calcite -> unverified: no Rust Calcite; candidates sqlparser 0.64.0 or the DataFusion unparser, dialect SQL parity not checked
- `kpi:rust-support-for-shared-dependencies`: TK. an input is unknown: unverified families: io.substrait.isthmus, net.snowflake.client, org.apache.calcite

## New gap rows

The drift check found 10 rows on the live page that the snapshot lacks. Two apply to this service. Neither is a blocker, and the score does not count either one.

- Data-lake crate version alignment (Watch Item, Medium): applies. A Rust port needs `arrow`, `arrow-ipc` and `parquet`, and `datafusion-substrait` if chosen, and all of them must stay on one arrow-rs major. Today's line is 60.1.0.
- Rust toolchain (nightly, cross-compilation, Lambda packaging) (Watch Item, Medium): applies in part. The CargoBrazil packaging limits do not bind a crates.io build, but the Lambda bootstrap and target-architecture constraints still apply.
- Legacy DynamoDB and S3 encryption clients: does not apply. Spill encryption is the SDK's own AES-GCM block format (`AesGcmBlockCrypto`), not the S3 Encryption Client.
- Iceberg table format, Ion parity, LMS GetStrings, Safe Dynamic Config, Strands on AgentCore, Tardigrade listener rules, and AWS Auth in smithy-rs: not used by these modules.

The page also dropped three rows that the snapshot still lists: Axon, BigDecimal over RPC v2 CBOR, and Disco for Rust. All three are now on Resolved Library Gaps, and none applies here.

## Caveats (not scored as gaps)

None.

## Platform caveats (apply to every Rust service, not scored)

- Load Shedding (Tower/SmithyRs) (TK, impact High, tracker TK): no page-stated path
- BuilderHub Rust Template (TK, impact High, tracker TK): Start from the BuilderHub Smithy Rust Service on EC2 or Lambda template, or scaffold with rustify-scaffold-rust-service and rustify-scaffold-cdk-infra.
- Hermetic Shadow Testing (TK, impact Medium, tracker TK): no page-stated path
- Coral behavior parity in smithy-rs (TK, impact TK, tracker TK): no page-stated path
- Rust security auditing (TK, impact TK, tracker TK): no page-stated path

## Provenance (scorer)

- Gap snapshot: scripts/library-gaps.json, dated 2026-11-02, sha256 5e6e9dfc20f7
- Scanned tree sha256 ac8832c060cf (sources athena-federation-sdk/src/main/java, athena-jdbc/src/main/java, athena-postgresql/src/main/java, athena-snowflake/src/main/java); compare with the current tree before reusing this report
- Live-page drift check: 0 changed, 10 new, 3 removed rows, 0 workaround quotes gone; live values used

## Provenance

| Item | Value |
|---|---|
| Source tree | main: `athena-federation-sdk/src/main/java`, `athena-jdbc/src/main/java`, `athena-postgresql/src/main/java`, `athena-snowflake/src/main/java`; test: the matching `src/test/java` roots |
| Workspace / version set | Not a Brazil workspace (`brazil workspace show`: "Directory isn't within a workspace"). Maven 3.9.9 build, OpenJDK 17.0.21 |
| map-dependencies run | `.rustify/map-dependencies/dependency-map.md`, mode triage, 2026-11-17, fresh run. The closure comes from `mvn -o dependency:tree` per module, converted to brazil-path shapes by `/tmp/rustify-tools/mvn2bp.py`. The import scan ran over a symlink union of the four main trees (`.rustify/map-dependencies/src-union`) because `scan-imports` takes one root |
| Gap page | `.rustify/readiness/gaps-live.md`, transcribed from the CommonLibraryGaps REST content (version 11.1, modified 2026-11-07T00:30:33Z), fetched 2026-11-17 through ReadInternalWebsites. Midway curl was unavailable (401) |
| Commands | `python3 <skill>/scripts/score_readiness.py template --service AthenaQueryFederation --out .rustify/readiness/inspection.json` |
| | `python3 <skill>/scripts/gap_lookup.py drift --wiki .rustify/readiness/gaps-live.md --out .rustify/readiness/gaps-drift.json` |
| | `python3 <map-skill>/scripts/map_dependencies.py pipeline --src .rustify/map-dependencies/src-union --work .rustify/map-dependencies --promotions .rustify/map-dependencies/promotions.tsv --out .rustify/map-dependencies --bp-all raw/all.txt --bp-graph raw/graph.txt --bp-direct raw/direct.txt --bp-testdirect raw/test.txt --bp-rundirect raw/run.txt --bp-tooldirect raw/tool.txt` |
| | `python3 <skill>/scripts/gap_lookup.py seed --work .rustify/map-dependencies --wiki .rustify/readiness/gaps-live.md --out .rustify/readiness/families.seed.json` |
| | `python3 <skill>/scripts/gap_lookup.py apply --families .rustify/readiness/families.seed.json --statuses .rustify/readiness/statuses.tsv --out .rustify/readiness/families.json` |
| | `python3 <skill>/scripts/scan_codebase.py --src athena-federation-sdk/src/main/java --src athena-jdbc/src/main/java --src athena-postgresql/src/main/java --src athena-snowflake/src/main/java --tests athena-federation-sdk/src/test/java --tests athena-jdbc/src/test/java --tests athena-postgresql/src/test/java --tests athena-snowflake/src/test/java --root . --out .rustify/readiness/scan.json` (one run over all four roots) |
| | `python3 <skill>/scripts/score_readiness.py score --inspection .rustify/readiness/inspection.json --families .rustify/readiness/families.json --scan .rustify/readiness/scan.json --out .rustify/readiness/readiness.json --markdown .rustify/readiness/readiness-sections.md` |
| `cargo brazil` probe | unavailable ("error: no such command: `brazil`"). Local cargo 1.96.0 used instead, recorded as an adaptation |
