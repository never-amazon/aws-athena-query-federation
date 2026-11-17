package com.amazonaws.athena.parity.wire;

import com.amazonaws.athena.connector.lambda.data.Block;
import com.amazonaws.athena.connector.lambda.data.BlockAllocator;
import com.amazonaws.athena.connector.lambda.data.BlockAllocatorImpl;
import com.amazonaws.athena.connector.lambda.data.BlockUtils;
import com.amazonaws.athena.connector.lambda.data.SchemaBuilder;
import com.amazonaws.athena.connector.lambda.domain.Split;
import com.amazonaws.athena.connector.lambda.domain.TableName;
import com.amazonaws.athena.connector.lambda.domain.predicate.AllOrNoneValueSet;
import com.amazonaws.athena.connector.lambda.domain.predicate.Constraints;
import com.amazonaws.athena.connector.lambda.domain.predicate.EquatableValueSet;
import com.amazonaws.athena.connector.lambda.domain.predicate.OrderByField;
import com.amazonaws.athena.connector.lambda.domain.predicate.QueryPlan;
import com.amazonaws.athena.connector.lambda.domain.predicate.Range;
import com.amazonaws.athena.connector.lambda.domain.predicate.SortedRangeSet;
import com.amazonaws.athena.connector.lambda.domain.predicate.ValueSet;
import com.amazonaws.athena.connector.lambda.domain.predicate.expression.ConstantExpression;
import com.amazonaws.athena.connector.lambda.domain.predicate.expression.FederationExpression;
import com.amazonaws.athena.connector.lambda.domain.predicate.expression.FunctionCallExpression;
import com.amazonaws.athena.connector.lambda.domain.predicate.expression.VariableExpression;
import com.amazonaws.athena.connector.lambda.domain.predicate.functions.StandardFunctions;
import com.amazonaws.athena.connector.lambda.domain.spill.S3SpillLocation;
import com.amazonaws.athena.connector.lambda.metadata.GetDataSourceCapabilitiesRequest;
import com.amazonaws.athena.connector.lambda.metadata.GetSplitsRequest;
import com.amazonaws.athena.connector.lambda.metadata.GetTableLayoutRequest;
import com.amazonaws.athena.connector.lambda.metadata.GetTableRequest;
import com.amazonaws.athena.connector.lambda.metadata.ListSchemasRequest;
import com.amazonaws.athena.connector.lambda.metadata.ListTablesRequest;
import com.amazonaws.athena.connector.lambda.records.ReadRecordsRequest;
import com.amazonaws.athena.connector.lambda.request.FederationRequest;
import com.amazonaws.athena.connector.lambda.request.PingRequest;
import com.amazonaws.athena.connector.lambda.security.FederatedIdentity;
import com.amazonaws.athena.connector.lambda.security.LocalKeyFactory;
import com.amazonaws.athena.connector.lambda.serde.VersionedObjectMapperFactory;
import com.amazonaws.athena.connector.lambda.udf.UserDefinedFunctionRequest;
import com.amazonaws.athena.connector.lambda.udf.UserDefinedFunctionType;
import com.amazonaws.athena.connectors.jdbc.splits.IntegerSplitter;
import com.amazonaws.athena.connectors.jdbc.splits.SplitInfo;
import com.amazonaws.athena.connectors.jdbc.splits.SplitRange;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.SerializationFeature;
import com.fasterxml.jackson.databind.node.ArrayNode;
import com.fasterxml.jackson.databind.node.ObjectNode;
import io.substrait.isthmus.SqlToSubstrait;
import io.substrait.plan.PlanProtoConverter;
import org.apache.arrow.vector.types.DateUnit;
import org.apache.arrow.vector.types.FloatingPointPrecision;
import org.apache.arrow.vector.types.Types;
import org.apache.arrow.vector.types.pojo.ArrowType;
import org.apache.arrow.vector.types.pojo.Schema;
import org.apache.calcite.config.CalciteConnectionConfig;
import org.apache.calcite.config.CalciteConnectionProperty;
import org.apache.calcite.jdbc.CalciteSchema;
import org.apache.calcite.prepare.CalciteCatalogReader;
import org.apache.calcite.rel.type.RelDataType;
import org.apache.calcite.rel.type.RelDataTypeFactory;
import org.apache.calcite.schema.Table;
import org.apache.calcite.schema.impl.AbstractSchema;
import org.apache.calcite.schema.impl.AbstractTable;
import org.apache.calcite.sql.dialect.AnsiSqlDialect;
import org.apache.calcite.sql.type.SqlTypeFactoryImpl;
import org.apache.calcite.sql.type.SqlTypeName;
import org.apache.calcite.util.Pair;

import java.io.IOException;
import java.math.BigDecimal;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Base64;
import java.util.Collections;
import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.function.Function;
import java.util.stream.Stream;

public final class CorpusGen
{
    static final int[] VERSIONS = {1, 2, 3, 4, 5, 6};
    static final String CATALOG = "parity_pg";
    static final String MUX_CATALOG = "pgmux";
    static final String MUX_DEFAULT_CATALOG = "lambda:parity-pg-fn";
    static final String SPILL_BUCKET = "parity-spill-bucket";
    static final String SPILL_PREFIX = "athena-spill";
    static final long BIG_BLOCK = 100_000_000L;
    static final String QPT_SIGNATURE = "SYSTEM.QUERY";
    static final String SECRET_CONNECTION = "postgres://jdbc:postgresql://{{PARITY_PG_HOSTPORT}}/paritydb?${parity-pg-secret}";
    static final String MISSING_SECRET_CONNECTION = "postgres://jdbc:postgresql://{{PARITY_PG_HOSTPORT}}/paritydb?${parity-missing-secret}";

    private static final ObjectMapper META = new ObjectMapper().enable(SerializationFeature.INDENT_OUTPUT);

    private final BlockAllocator allocator = new BlockAllocatorImpl("parity-corpus");
    private final List<CaseSpec> cases = new ArrayList<>();
    private final Map<String, String> raws = new TreeMap<>();

    private static final class CaseSpec
    {
        final String group;
        final String variant;
        final int minVersion;
        final String handler;
        final Map<String, String> env;
        final String description;
        final Function<String, FederationRequest> request;

        CaseSpec(String group, String variant, int minVersion, String handler, Map<String, String> env, String description,
                Function<String, FederationRequest> request)
        {
            this.group = group;
            this.variant = variant;
            this.minVersion = minVersion;
            this.handler = handler;
            this.env = env;
            this.description = description;
            this.request = request;
        }
    }

    private CorpusGen() {}

    public static void main(String[] args) throws Exception
    {
        if (args.length != 1) {
            System.err.println("usage: CorpusGen <corpus dir>");
            System.exit(2);
        }
        Path out = Paths.get(args[0]);
        CorpusGen gen = new CorpusGen();
        gen.define();
        gen.write(out);
        System.out.flush();
        System.exit(0);
    }

    private void add(String group, String variant, int minVersion, String description, Function<String, FederationRequest> request)
    {
        add(group, variant, minVersion, "single", Map.of(), description, request);
    }

    private void add(String group, String variant, int minVersion, String handler, Map<String, String> env, String description,
            Function<String, FederationRequest> request)
    {
        cases.add(new CaseSpec(group, variant, minVersion, handler, env, description, request));
    }

    private void write(Path out) throws Exception
    {
        if (Files.exists(out)) {
            try (Stream<Path> walk = Files.walk(out)) {
                walk.sorted(Comparator.reverseOrder()).forEach(p -> p.toFile().delete());
            }
        }
        Files.createDirectories(out);
        Set<String> names = new LinkedHashSet<>();
        int count = 0;
        for (CaseSpec spec : cases) {
            for (int version : VERSIONS) {
                if (version < spec.minVersion) {
                    continue;
                }
                String base = spec.group + "__" + spec.variant;
                String name = base + "__v" + version;
                if (!names.add(name)) {
                    throw new IllegalStateException("duplicate case " + name);
                }
                FederationRequest request = spec.request.apply("parity-" + base.replace("__", "-").replace('_', '-'));
                byte[] bytes;
                try {
                    ObjectMapper mapper = VersionedObjectMapperFactory.create(allocator, version);
                    bytes = mapper.writeValueAsBytes(request);
                }
                catch (Exception e) {
                    throw new IllegalStateException("cannot serialize " + name + ": " + e, e);
                }
                Path dir = out.resolve(name);
                Files.createDirectories(dir);
                Files.write(dir.resolve("request.json"), bytes);
                ObjectNode meta = META.createObjectNode();
                meta.put("name", name);
                meta.put("group", spec.group);
                meta.put("variant", spec.variant);
                meta.put("requestType", requestType(request));
                meta.put("serdeVersion", version);
                meta.put("handler", spec.handler);
                ObjectNode env = meta.putObject("env");
                new TreeMap<>(spec.env).forEach(env::put);
                meta.put("description", spec.description);
                Files.write(dir.resolve("case.json"), (META.writeValueAsString(meta) + "\n").getBytes(StandardCharsets.UTF_8));
                count++;
            }
        }
        for (Map.Entry<String, String> raw : raws.entrySet()) {
            Path dir = out.resolve(raw.getKey());
            Files.createDirectories(dir);
            Files.write(dir.resolve("request.json"), raw.getValue().getBytes(StandardCharsets.UTF_8));
            ObjectNode meta = META.createObjectNode();
            meta.put("name", raw.getKey());
            meta.put("group", "raw");
            meta.put("variant", raw.getKey().substring("raw__".length()));
            meta.put("requestType", "raw");
            meta.putNull("serdeVersion");
            meta.put("handler", "single");
            meta.putObject("env");
            meta.put("description", "Hand-shaped request bytes that no serializer produces (error path)");
            Files.write(dir.resolve("case.json"), (META.writeValueAsString(meta) + "\n").getBytes(StandardCharsets.UTF_8));
            count++;
        }
        System.out.println("wrote " + count + " cases from " + (cases.size() + raws.size()) + " specs to " + out);
    }

    private static String requestType(FederationRequest request)
    {
        return request.getClass().getSimpleName();
    }

    static FederatedIdentity identity()
    {
        return identity(Map.of());
    }

    static FederatedIdentity identity(Map<String, String> configOptions)
    {
        return new FederatedIdentity("arn:aws:iam::123456789012:user/parity", "123456789012", Map.of(), List.of(), configOptions);
    }

    private Constraints constraints(Map<String, ValueSet> summary)
    {
        return new Constraints(summary, List.of(), List.of(), Constraints.DEFAULT_NO_LIMIT, Map.of(), null);
    }

    private Constraints constraints(Map<String, ValueSet> summary, List<FederationExpression> expressions, List<OrderByField> orderBy, long limit)
    {
        return new Constraints(summary, expressions, orderBy, limit, Map.of(), null);
    }

    private static Constraints qptConstraints(Map<String, String> qpt)
    {
        return new Constraints(Map.of(), List.of(), List.of(), Constraints.DEFAULT_NO_LIMIT, qpt, null);
    }

    private static Map<String, String> qptArgs(String query)
    {
        Map<String, String> args = new LinkedHashMap<>();
        args.put("schemaFunctionName", QPT_SIGNATURE);
        args.put("QUERY", query);
        return args;
    }

    private static S3SpillLocation spill(String queryId, String splitId)
    {
        return S3SpillLocation.newBuilder()
                .withBucket(SPILL_BUCKET)
                .withPrefix(SPILL_PREFIX)
                .withQueryId(queryId)
                .withSplitId(splitId)
                .withIsDirectory(true)
                .build();
    }

    private static Split split(String queryId, String partitionSchema, String partitionName)
    {
        return Split.newBuilder(spill(queryId, "split-0001"), null)
                .add("partition_schema_name", partitionSchema)
                .add("partition_name", partitionName)
                .build();
    }

    private static Split encryptedSplit(String queryId, String partitionSchema, String partitionName)
    {
        return Split.newBuilder(spill(queryId, "split-0001"), new LocalKeyFactory().create())
                .add("partition_schema_name", partitionSchema)
                .add("partition_name", partitionName)
                .build();
    }

    static ArrowType utf8()
    {
        return Types.MinorType.VARCHAR.getType();
    }

    static Schema partitionSchema()
    {
        return SchemaBuilder.newBuilder()
                .addField("partition_schema_name", utf8())
                .addField("partition_name", utf8())
                .build();
    }

    private Block partitions(String[][] rows)
    {
        Block block = allocator.createBlock(partitionSchema());
        for (int i = 0; i < rows.length; i++) {
            BlockUtils.setValue(block.getFieldVector("partition_schema_name"), i, rows[i][0]);
            BlockUtils.setValue(block.getFieldVector("partition_name"), i, rows[i][1]);
        }
        block.setRowCount(rows.length);
        return block;
    }

    private static Set<String> partitionCols()
    {
        return new LinkedHashSet<>(List.of("partition_schema_name", "partition_name"));
    }

    static List<String> splitClauses(int low, int high, String column)
    {
        IntegerSplitter splitter = new IntegerSplitter(new SplitInfo<>(new SplitRange<>(low, high), column, java.sql.Types.INTEGER, 20));
        List<String> clauses = new ArrayList<>();
        while (splitter.hasNext()) {
            clauses.add(splitter.nextRangeClause());
        }
        return clauses;
    }

    private void define() throws Exception
    {
        definePing();
        defineUnsupported();
        defineCapabilities();
        defineListSchemas();
        defineListTables();
        defineGetTable();
        defineGetTableLayout();
        defineGetSplits();
        defineReadRecords();
        defineExtra();
    }

    private void defineUnsupported()
    {
        add("udf", "not_served", 1, "UserDefinedFunctionRequest, which the PostgreSQL handler does not serve (error)",
                q -> new UserDefinedFunctionRequest(identity(), BlockUtils.newBlock(allocator, "col1", Types.MinorType.INT.getType(), List.of(1, 2)),
                        SchemaBuilder.newBuilder().addField("out", Types.MinorType.INT.getType()).build(), "add_one", UserDefinedFunctionType.SCALAR));
        raws.put("raw__unknown_type", "{\"@type\":\"NoSuchRequest\",\"identity\":{\"id\":\"UNKNOWN\",\"principal\":\"UNKNOWN\",\"account\":\"123456789012\",\"arn\":\"arn:aws:iam::123456789012:user/parity\",\"tags\":{},\"groups\":[],\"configOptions\":{}},\"catalogName\":\"parity_pg\",\"queryId\":\"parity-raw-unknown-type\"}");
        raws.put("raw__not_json", "this is not json");
        raws.put("raw__empty_object", "{}");
        raws.put("raw__ping_missing_catalog", "{\"@type\":\"PingRequest\",\"identity\":{\"id\":\"UNKNOWN\",\"principal\":\"UNKNOWN\",\"account\":\"123456789012\",\"arn\":\"arn:aws:iam::123456789012:user/parity\",\"tags\":{},\"groups\":[],\"configOptions\":{}},\"queryId\":\"parity-raw-ping\"}");
    }

    private void definePing()
    {
        add("ping", "single", 1, "PingRequest against the single-instance handler",
                q -> new PingRequest(identity(), CATALOG, q));
        add("ping", "mux", 1, "mux", Map.of(), "PingRequest against the mux handler",
                q -> new PingRequest(identity(), MUX_CATALOG, q));
    }

    private void defineCapabilities()
    {
        add("capabilities", "default", 4, "GetDataSourceCapabilities with default config",
                q -> new GetDataSourceCapabilitiesRequest(identity(), q, CATALOG));
        add("capabilities", "non_default_collate", 4, "single", Map.of("non_default_collate", "true"),
                "GetDataSourceCapabilities with non_default_collate hint",
                q -> new GetDataSourceCapabilitiesRequest(identity(), q, CATALOG));
        add("capabilities", "qpt_disabled", 4, "single", Map.of("enable_query_passthrough", "false"),
                "GetDataSourceCapabilities with query passthrough turned off",
                q -> new GetDataSourceCapabilitiesRequest(identity(), q, CATALOG));
        add("capabilities", "mux_named", 4, "mux", Map.of(), "GetDataSourceCapabilities through the mux handler",
                q -> new GetDataSourceCapabilitiesRequest(identity(), q, MUX_CATALOG));
    }

    private void defineListSchemas()
    {
        add("list_schemas", "single", 1, "ListSchemas on the single handler",
                q -> new ListSchemasRequest(identity(), q, CATALOG));
        add("list_schemas", "secret_credentials", 1, "single", Map.of("default", SECRET_CONNECTION),
                "ListSchemas with the connection string credentials taken from the Secrets Manager stub",
                q -> new ListSchemasRequest(identity(), q, CATALOG));
        add("list_schemas", "secret_missing", 1, "single", Map.of("default", MISSING_SECRET_CONNECTION),
                "ListSchemas with a connection string secret the Secrets Manager stub does not hold (error)",
                q -> new ListSchemasRequest(identity(), q, CATALOG));
        add("list_schemas", "mux_default", 1, "mux", Map.of(), "ListSchemas on the mux default catalog",
                q -> new ListSchemasRequest(identity(), q, MUX_DEFAULT_CATALOG));
        add("list_schemas", "mux_named", 1, "mux", Map.of(), "ListSchemas on a named mux catalog",
                q -> new ListSchemasRequest(identity(), q, MUX_CATALOG));
        add("list_schemas", "mux_unknown_catalog", 1, "mux", Map.of(), "ListSchemas on a catalog the mux does not know (error)",
                q -> new ListSchemasRequest(identity(), q, "not_registered"));
    }

    private void defineListTables()
    {
        int unlimited = ListTablesRequest.UNLIMITED_PAGE_SIZE_VALUE;
        add("list_tables", "parity_all", 1, "ListTables without pagination, includes views and a materialized view",
                q -> new ListTablesRequest(identity(), q, CATALOG, "parity", null, unlimited));
        add("list_tables", "pages_all", 1, "ListTables without pagination on a schema with 26 entries",
                q -> new ListTablesRequest(identity(), q, CATALOG, "parity_pages", null, unlimited));
        add("list_tables", "pages_first", 1, "ListTables page 1 of size 10",
                q -> new ListTablesRequest(identity(), q, CATALOG, "parity_pages", null, 10));
        add("list_tables", "pages_second", 1, "ListTables page 2 of size 10 via token",
                q -> new ListTablesRequest(identity(), q, CATALOG, "parity_pages", "10", 10));
        add("list_tables", "pages_last_partial", 1, "ListTables last partial page",
                q -> new ListTablesRequest(identity(), q, CATALOG, "parity_pages", "20", 10));
        add("list_tables", "pages_exact", 1, "ListTables page whose size equals the remaining count",
                q -> new ListTablesRequest(identity(), q, CATALOG, "parity_pages", "16", 10));
        add("list_tables", "pages_beyond_end", 1, "ListTables with a token past the end",
                q -> new ListTablesRequest(identity(), q, CATALOG, "parity_pages", "100", 10));
        add("list_tables", "pages_token_no_size", 1, "ListTables with a token and unlimited page size",
                q -> new ListTablesRequest(identity(), q, CATALOG, "parity_pages", "5", unlimited));
        add("list_tables", "parity_paged_mv", 1, "ListTables paginated on the schema that holds a materialized view",
                q -> new ListTablesRequest(identity(), q, CATALOG, "parity", null, 50));
        add("list_tables", "mixed_lower_input", 1, "ListTables with a lower-case name for a mixed-case schema",
                q -> new ListTablesRequest(identity(), q, CATALOG, "mixedschema", null, unlimited));
        add("list_tables", "mixed_exact_casing_none", 1, "single", Map.of("casing_mode", "none"),
                "ListTables with exact mixed-case schema and casing_mode none",
                q -> new ListTablesRequest(identity(), q, CATALOG, "MixedSchema", null, unlimited));
        add("list_tables", "casing_lower", 1, "single", Map.of("casing_mode", "lower"),
                "ListTables with casing_mode lower on a mixed-case schema name",
                q -> new ListTablesRequest(identity(), q, CATALOG, "MixedSchema", null, unlimited));
        add("list_tables", "casing_upper", 1, "single", Map.of("casing_mode", "upper"),
                "ListTables with casing_mode upper",
                q -> new ListTablesRequest(identity(), q, CATALOG, "parity", null, unlimited));
        add("list_tables", "casing_invalid", 1, "single", Map.of("casing_mode", "sideways"),
                "ListTables with an invalid casing_mode (error)",
                q -> new ListTablesRequest(identity(), q, CATALOG, "parity", null, unlimited));
        add("list_tables", "empty_schema", 1, "ListTables on an empty schema",
                q -> new ListTablesRequest(identity(), q, CATALOG, "empty_schema", null, unlimited));
        add("list_tables", "unknown_schema", 1, "ListTables on a schema that does not exist (error)",
                q -> new ListTablesRequest(identity(), q, CATALOG, "no_such_schema", null, unlimited));
        add("list_tables", "unknown_schema_paged", 1, "ListTables paginated on a schema that does not exist (error)",
                q -> new ListTablesRequest(identity(), q, CATALOG, "no_such_schema", null, 10));
        add("list_tables", "bad_token", 1, "ListTables with a token that is not a number (error)",
                q -> new ListTablesRequest(identity(), q, CATALOG, "parity_pages", "abc", 10));
        add("list_tables", "mux_named", 1, "mux", Map.of(), "ListTables on a named mux catalog",
                q -> new ListTablesRequest(identity(), q, MUX_CATALOG, "parity", null, unlimited));
    }

    private void defineGetTable()
    {
        String[][] tables = {
            {"all_types", "parity", "all_types"},
            {"sales_partitioned", "parity", "sales"},
            {"sales_child", "parity", "sales_2023"},
            {"view", "parity", "all_types_view"},
            {"materialized_view", "parity", "sales_summary_mv"},
            {"small_pk", "parity", "small_pk"},
            {"uuid_pk", "parity", "uuid_pk"},
            {"no_pk", "parity", "no_pk"},
            {"empty_table", "parity", "empty_table"},
            {"camel_lower_input", "parity", "cameltable"},
            {"mixed_lower_input", "mixedschema", "mixedtable"},
            {"mixed_schema_lower_table", "mixedschema", "lower_table"},
            {"unknown_table", "parity", "no_such_table"},
            {"unknown_schema", "no_such_schema", "all_types"},
        };
        for (String[] t : tables) {
            add("get_table", t[0], 1, "GetTable " + t[1] + "." + t[2],
                    q -> new GetTableRequest(identity(), q, CATALOG, new TableName(t[1], t[2]), Map.of()));
        }
        add("get_table", "mixed_exact_casing_none", 1, "single", Map.of("casing_mode", "none"),
                "GetTable with exact mixed-case names and casing_mode none",
                q -> new GetTableRequest(identity(), q, CATALOG, new TableName("MixedSchema", "MixedTable"), Map.of()));
        add("get_table", "mixed_lower_casing_none", 1, "single", Map.of("casing_mode", "none"),
                "GetTable with lower-case names and casing_mode none (table not found)",
                q -> new GetTableRequest(identity(), q, CATALOG, new TableName("mixedschema", "mixedtable"), Map.of()));
        add("get_table", "casing_lower", 1, "single", Map.of("casing_mode", "lower"),
                "GetTable with casing_mode lower on a mixed-case table",
                q -> new GetTableRequest(identity(), q, CATALOG, new TableName("parity", "CamelTable"), Map.of()));
        add("get_table", "secret_credentials", 1, "single", Map.of("default", SECRET_CONNECTION),
                "GetTable with credentials from the Secrets Manager stub",
                q -> new GetTableRequest(identity(), q, CATALOG, new TableName("parity", "small_pk"), Map.of()));
        add("get_table", "default_scale", 1, "single", Map.of("default_scale", "6"),
                "GetTable all_types with default_scale for unbounded numeric",
                q -> new GetTableRequest(identity(), q, CATALOG, new TableName("parity", "all_types"), Map.of()));
        add("get_table", "mux_named", 1, "mux", Map.of(), "GetTable through a named mux catalog",
                q -> new GetTableRequest(identity(), q, MUX_CATALOG, new TableName("parity", "small_pk"), Map.of()));
        add("get_table", "mux_unknown_catalog", 1, "mux", Map.of(), "GetTable through an unknown mux catalog (error)",
                q -> new GetTableRequest(identity(), q, "not_registered", new TableName("parity", "small_pk"), Map.of()));

        add("get_table", "qpt_scalar_columns", 5, "GetTable query passthrough with scalar columns",
                q -> new GetTableRequest(identity(), q, CATALOG, new TableName("system", "query"),
                        qptArgs("SELECT id, smallint_col, bigint_col, numeric_scaled, numeric_unbounded, real_col, double_col, text_col, char_col, bool_col, date_col, ts_col, tstz_col, bytea_col FROM parity.all_types WHERE id < 50")));
        add("get_table", "qpt_complex_columns", 5, "GetTable query passthrough with arrays, uuid, json and time",
                q -> new GetTableRequest(identity(), q, CATALOG, new TableName("system", "query"),
                        qptArgs("SELECT int_arr, text_arr, numeric_arr, uuid_col, json_col, time_col FROM parity.all_types")));
        add("get_table", "qpt_alias_and_expr", 5, "GetTable query passthrough with aliases and computed columns",
                q -> new GetTableRequest(identity(), q, CATALOG, new TableName("system", "query"),
                        qptArgs("SELECT id AS \"Renamed\", int_col * 2 AS doubled, count(*) OVER () AS total FROM parity.all_types")));
        add("get_table", "qpt_bad_signature", 5, "GetTable query passthrough with a wrong function signature (error)",
                q -> new GetTableRequest(identity(), q, CATALOG, new TableName("system", "query"),
                        Map.of("schemaFunctionName", "SYSTEM.OTHER", "QUERY", "SELECT 1")));
        add("get_table", "qpt_missing_query", 5, "GetTable query passthrough without the QUERY argument (error)",
                q -> new GetTableRequest(identity(), q, CATALOG, new TableName("system", "query"),
                        Map.of("schemaFunctionName", QPT_SIGNATURE)));
        add("get_table", "qpt_invalid_sql", 5, "GetTable query passthrough with SQL the database rejects (error)",
                q -> new GetTableRequest(identity(), q, CATALOG, new TableName("system", "query"),
                        qptArgs("SELECT no_such_column FROM parity.all_types")));
    }

    private void defineGetTableLayout()
    {
        add("get_table_layout", "no_partitions", 1, "GetTableLayout on a table without partitions",
                q -> new GetTableLayoutRequest(identity(), q, CATALOG, new TableName("parity", "all_types"),
                        constraints(Map.of()), Schemas.allTypes(), partitionCols()));
        add("get_table_layout", "partitioned", 1, "GetTableLayout on a declaratively partitioned table",
                q -> new GetTableLayoutRequest(identity(), q, CATALOG, new TableName("parity", "sales"),
                        constraints(Map.of()), Schemas.salesWithPartitionCols(), partitionCols()));
        add("get_table_layout", "partitioned_with_constraint", 1, "GetTableLayout on a partitioned table with a constraint the connector ignores",
                q -> new GetTableLayoutRequest(identity(), q, CATALOG, new TableName("parity", "sales"),
                        constraints(Map.of("sale_date", SortedRangeSet.copyOf(Types.MinorType.DATEDAY.getType(),
                                List.of(Range.greaterThanOrEqual(allocator, Types.MinorType.DATEDAY.getType(), (int) LocalDate.of(2024, 1, 1).toEpochDay())), false))),
                        Schemas.salesWithPartitionCols(), partitionCols()));
        add("get_table_layout", "mixed_case", 1, "GetTableLayout on a mixed-case table",
                q -> new GetTableLayoutRequest(identity(), q, CATALOG, new TableName("MixedSchema", "MixedTable"),
                        constraints(Map.of()), Schemas.mixedTable(), partitionCols()));
        add("get_table_layout", "unknown_table", 1, "GetTableLayout on a missing table (single wildcard partition)",
                q -> new GetTableLayoutRequest(identity(), q, CATALOG, new TableName("parity", "no_such_table"),
                        constraints(Map.of()), Schemas.smallPk(), partitionCols()));
        add("get_table_layout", "mux_named", 1, "mux", Map.of(), "GetTableLayout through a named mux catalog",
                q -> new GetTableLayoutRequest(identity(), q, MUX_CATALOG, new TableName("parity", "sales"),
                        constraints(Map.of()), Schemas.salesWithPartitionCols(), partitionCols()));
    }

    private void defineGetSplits()
    {
        String[][] all = {{"*", "*"}};
        String[][] salesParts = {{"parity", "sales_2023"}, {"parity", "sales_2024"}, {"parity", "sales_other"}};
        add("get_splits", "pk_splitter", 1, "GetSplits on all_types, the integer primary key gives 20 range splits",
                q -> splitsRequest(q, CATALOG, "parity", "all_types", partitions(all), null, constraints(Map.of()), identity()));
        add("get_splits", "partitions", 1, "GetSplits on the partitioned table, one split per partition",
                q -> splitsRequest(q, CATALOG, "parity", "sales", partitions(salesParts), null, constraints(Map.of()), identity()));
        add("get_splits", "partitions_continuation", 1, "GetSplits on the partitioned table with continuation token 1",
                q -> splitsRequest(q, CATALOG, "parity", "sales", partitions(salesParts), "1", constraints(Map.of()), identity()));
        add("get_splits", "partitions_continuation_end", 1, "GetSplits with a continuation token at the end",
                q -> splitsRequest(q, CATALOG, "parity", "sales", partitions(salesParts), "3", constraints(Map.of()), identity()));
        add("get_splits", "pk_splitter_continuation", 1, "GetSplits with wildcard partition and a continuation token",
                q -> splitsRequest(q, CATALOG, "parity", "all_types", partitions(all), "0", constraints(Map.of()), identity()));
        add("get_splits", "small_range", 1, "GetSplits on a table whose key range is too small to split",
                q -> splitsRequest(q, CATALOG, "parity", "small_pk", partitions(all), null, constraints(Map.of()), identity()));
        add("get_splits", "uuid_pk", 1, "GetSplits on a table with a UUID primary key (splitter skipped)",
                q -> splitsRequest(q, CATALOG, "parity", "uuid_pk", partitions(all), null, constraints(Map.of()), identity()));
        add("get_splits", "no_pk", 1, "GetSplits on a table without a primary key",
                q -> splitsRequest(q, CATALOG, "parity", "no_pk", partitions(all), null, constraints(Map.of()), identity()));
        add("get_splits", "empty_table", 1, "GetSplits on an empty table",
                q -> splitsRequest(q, CATALOG, "parity", "empty_table", partitions(all), null, constraints(Map.of()), identity()));
        add("get_splits", "mixed_case_pk", 1, "GetSplits on a mixed-case table with a mixed-case key",
                q -> splitsRequest(q, CATALOG, "parity", "CamelTable", partitions(all), null, constraints(Map.of()), identity()));
        add("get_splits", "catalog_casing_filter", 1, "GetSplits with CATALOG_CASING_FILTER in identity config options",
                q -> splitsRequest(q, CATALOG, "parity", "sales", partitions(salesParts), null, constraints(Map.of()),
                        identity(Map.of("CATALOG_CASING_FILTER", "UPPERCASE_ONLY"))));
        add("get_splits", "identity_spill_override", 1, "GetSplits with spill bucket and prefix taken from identity config options",
                q -> splitsRequest(q, CATALOG, "parity", "small_pk", partitions(all), null, constraints(Map.of()),
                        identity(Map.of("spill_bucket", "identity-bucket", "spill_prefix", "identity-prefix"))));
        add("get_splits", "spill_encryption_on", 1, "single", Map.of("disable_spill_encryption", "false"),
                "GetSplits on the partitioned table with spill encryption on, so each split carries a random key",
                q -> splitsRequest(q, CATALOG, "parity", "sales", partitions(salesParts), null, constraints(Map.of()), identity()));
        add("get_splits", "spill_encryption_on_pk", 1, "single", Map.of("disable_spill_encryption", "false"),
                "GetSplits on a primary key range table with spill encryption on",
                q -> splitsRequest(q, CATALOG, "parity", "all_types", partitions(all), null, constraints(Map.of()), identity()));
        add("get_splits", "mux_named", 1, "mux", Map.of(), "GetSplits through a named mux catalog",
                q -> splitsRequest(q, MUX_CATALOG, "parity", "sales", partitions(salesParts), null, constraints(Map.of()), identity()));
        add("get_splits", "qpt", 5, "GetSplits for a query passthrough request",
                q -> splitsRequest(q, CATALOG, "system", "query", partitions(all), null,
                        qptConstraints(qptArgs("SELECT id, text_col FROM parity.all_types WHERE id <= 5")), identity()));
    }

    private GetSplitsRequest splitsRequest(String queryId, String catalog, String schema, String table, Block partitions, String token,
            Constraints constraints, FederatedIdentity identity)
    {
        return new GetSplitsRequest(identity, queryId, catalog, new TableName(schema, table), partitions,
                List.of("partition_schema_name", "partition_name"), constraints, token);
    }

    private ReadRecordsRequest read(String queryId, TableName table, Schema schema, Split split, Constraints constraints)
    {
        return read(queryId, CATALOG, table, schema, split, constraints, BIG_BLOCK, BIG_BLOCK);
    }

    private ReadRecordsRequest read(String queryId, String catalog, TableName table, Schema schema, Split split, Constraints constraints,
            long maxBlock, long maxInline)
    {
        return new ReadRecordsRequest(identity(), catalog, queryId, table, schema, split, constraints, maxBlock, maxInline);
    }

    private void defineReadRecords() throws Exception
    {
        TableName allTypes = new TableName("parity", "all_types");
        ArrowType int32 = Types.MinorType.INT.getType();
        ArrowType int64 = Types.MinorType.BIGINT.getType();
        ArrowType float8 = Types.MinorType.FLOAT8.getType();
        ArrowType float4 = Types.MinorType.FLOAT4.getType();
        ArrowType int16 = Types.MinorType.SMALLINT.getType();
        ArrowType bool = Types.MinorType.BIT.getType();
        ArrowType dateDay = Types.MinorType.DATEDAY.getType();
        ArrowType dateMilli = Types.MinorType.DATEMILLI.getType();
        ArrowType dec184 = new ArrowType.Decimal(18, 4, 128);
        List<String> clauses = splitClauses(1, 3000, "\"id\"");

        add("read_records", "full_table", 1, "ReadRecords on all_types, every column, wildcard split",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of())));
        add("read_records", "pk_split_first", 1, "ReadRecords on the first primary key range split",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", clauses.get(0)), constraints(Map.of())));
        add("read_records", "pk_split_last", 1, "ReadRecords on the last primary key range split",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", clauses.get(clauses.size() - 1)), constraints(Map.of())));
        add("read_records", "projection", 1, "ReadRecords with a subset of columns",
                q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "text_col", "date_col"), split(q, "*", "*"), constraints(Map.of())));
        add("read_records", "no_columns", 1, "ReadRecords with an empty projection",
                q -> read(q, allTypes, SchemaBuilder.newBuilder().build(), split(q, "*", clauses.get(0)), constraints(Map.of())));
        add("read_records", "range_int", 1, "ReadRecords with 100 < int_col <= 500",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("int_col",
                        SortedRangeSet.copyOf(int32, List.of(Range.range(allocator, int32, 100, false, 500, true)), false)))));
        add("read_records", "range_multi_bigint", 1, "ReadRecords with two disjoint bigint ranges",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("bigint_col",
                        SortedRangeSet.copyOf(int64, List.of(
                                Range.lessThan(allocator, int64, -1_400_000_000L),
                                Range.greaterThanOrEqual(allocator, int64, 1_400_000_000L)), false)))));
        add("read_records", "equal_single", 1, "ReadRecords with int_col = 9",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("int_col",
                        SortedRangeSet.copyOf(int32, List.of(Range.equal(allocator, int32, 9)), false)))));
        add("read_records", "in_list_ranges", 1, "ReadRecords with int_col IN (2, 9, 16) as single-value ranges",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("int_col",
                        SortedRangeSet.copyOf(int32, List.of(Range.equal(allocator, int32, 2), Range.equal(allocator, int32, 9),
                                Range.equal(allocator, int32, 16)), false)))));
        add("read_records", "in_list_varchar", 1, "ReadRecords with varchar_col IN ('alpha', 'gamma') as single-value ranges",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("varchar_col",
                        SortedRangeSet.copyOf(utf8(), List.of(Range.equal(allocator, utf8(), "alpha"), Range.equal(allocator, utf8(), "gamma")), false)))));
        add("read_records", "in_list_varchar_or_null", 1, "ReadRecords with varchar_col IN ('beta') OR varchar_col IS NULL",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", clauses.get(7)), constraints(Map.of("varchar_col",
                        SortedRangeSet.copyOf(utf8(), List.of(Range.equal(allocator, utf8(), "beta")), true)))));
        add("read_records", "equal_char_range", 1, "ReadRecords with char_col = 'c5' as a single-value range on a padded char column",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("char_col",
                        SortedRangeSet.copyOf(utf8(), List.of(Range.equal(allocator, utf8(), "c5")), false)))));
        add("read_records", "equatable_allow", 1, "ReadRecords with varchar_col IN ('alpha', 'gamma') as an EquatableValueSet",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("varchar_col",
                        EquatableValueSet.newBuilder(allocator, utf8(), true, false).add("alpha").add("gamma").build()))));
        add("read_records", "equatable_deny", 1, "ReadRecords with varchar_col NOT IN ('alpha') as an EquatableValueSet",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("varchar_col",
                        EquatableValueSet.newBuilder(allocator, utf8(), false, false).add("alpha").build()))));
        add("read_records", "equatable_allow_null", 1, "ReadRecords with varchar_col IN ('beta') OR NULL",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("varchar_col",
                        EquatableValueSet.newBuilder(allocator, utf8(), true, true).add("beta").build()))));
        add("read_records", "only_null", 1, "ReadRecords with numeric_scaled IS NULL",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("numeric_scaled",
                        SortedRangeSet.onlyNull(dec184)))));
        add("read_records", "not_null", 1, "ReadRecords with text_col IS NOT NULL",
                q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "text_col"), split(q, "*", clauses.get(1)),
                        constraints(Map.of("text_col", SortedRangeSet.notNull(allocator, utf8())))));
        add("read_records", "range_or_null", 1, "ReadRecords with int_col < -4900 OR int_col IS NULL",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("int_col",
                        SortedRangeSet.copyOf(int32, List.of(Range.lessThan(allocator, int32, -4900)), true)))));
        add("read_records", "none_value_set", 1, "ReadRecords with an AllOrNoneValueSet that matches nothing",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("int_col",
                        new AllOrNoneValueSet(int32, false, false)))));
        add("read_records", "all_value_set", 1, "ReadRecords with an AllOrNoneValueSet that matches everything",
                q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "int_col"), split(q, "*", clauses.get(2)),
                        constraints(Map.of("int_col", new AllOrNoneValueSet(int32, true, true)))));
        add("read_records", "range_date", 1, "ReadRecords with a date range",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("date_col",
                        SortedRangeSet.copyOf(dateDay, List.of(Range.range(allocator, dateDay,
                                (int) LocalDate.of(1965, 1, 1).toEpochDay(), true, (int) LocalDate.of(1972, 6, 30).toEpochDay(), false)), false)))));
        add("read_records", "range_timestamp", 1, "ReadRecords with a timestamp range",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("ts_col",
                        SortedRangeSet.copyOf(dateMilli, List.of(Range.range(allocator, dateMilli,
                                LocalDateTime.of(1966, 1, 1, 0, 0), true, LocalDateTime.of(1967, 1, 1, 12, 30, 15), true)), false)))));
        add("read_records", "range_timestamptz", 1, "ReadRecords with a range on a timestamptz column",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("tstz_col",
                        SortedRangeSet.copyOf(dateMilli, List.of(Range.greaterThan(allocator, dateMilli,
                                LocalDateTime.of(2001, 3, 1, 0, 0))), false)))));
        add("read_records", "range_double", 1, "ReadRecords with a double range",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("double_col",
                        SortedRangeSet.copyOf(float8, List.of(Range.range(allocator, float8, 0.0D, true, 250.5D, false)), false)))));
        add("read_records", "range_real", 1, "ReadRecords with a real range",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("real_col",
                        SortedRangeSet.copyOf(float4, List.of(Range.lessThanOrEqual(allocator, float4, 10.5F)), false)))));
        add("read_records", "range_smallint", 1, "ReadRecords with a smallint range",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("smallint_col",
                        SortedRangeSet.copyOf(int16, List.of(Range.range(allocator, int16, (short) -15990, true, (short) -15900, true)), false)))));
        add("read_records", "range_decimal", 1, "ReadRecords with a decimal range",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("numeric_scaled",
                        SortedRangeSet.copyOf(dec184, List.of(Range.range(allocator, dec184, new BigDecimal("100.0000"), true,
                                new BigDecimal("2000.5000"), false)), false)))));
        add("read_records", "range_varchar", 1, "ReadRecords with a string range on text_col",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("text_col",
                        SortedRangeSet.copyOf(utf8(), List.of(Range.range(allocator, utf8(), "text_00100", true, "text_00200", false)), false)))));
        add("read_records", "equal_bool", 1, "ReadRecords with bool_col = true",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", clauses.get(3)), constraints(Map.of("bool_col",
                        SortedRangeSet.copyOf(bool, List.of(Range.equal(allocator, bool, true)), false)))));
        add("read_records", "equal_char", 1, "ReadRecords with char_col = 'c5' on a padded char column",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("char_col",
                        EquatableValueSet.newBuilder(allocator, utf8(), true, false).add("c5").build()))));
        add("read_records", "multi_column_equatable", 1, "ReadRecords with a range and an EquatableValueSet on two columns",
                q -> {
                    Map<String, ValueSet> summary = new LinkedHashMap<>();
                    summary.put("int_col", SortedRangeSet.copyOf(int32, List.of(Range.greaterThan(allocator, int32, 0)), false));
                    summary.put("varchar_col", EquatableValueSet.newBuilder(allocator, utf8(), true, false).add("beta").build());
                    return read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(summary));
                });
        add("read_records", "multi_column", 1, "ReadRecords with constraints on several columns",
                q -> {
                    Map<String, ValueSet> summary = new LinkedHashMap<>();
                    summary.put("int_col", SortedRangeSet.copyOf(int32, List.of(Range.greaterThan(allocator, int32, 0)), false));
                    summary.put("bool_col", SortedRangeSet.copyOf(bool, List.of(Range.equal(allocator, bool, false)), true));
                    summary.put("varchar_col", SortedRangeSet.copyOf(utf8(), List.of(Range.equal(allocator, utf8(), "beta"),
                            Range.equal(allocator, utf8(), "delta")), false));
                    return read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(summary));
                });
        add("read_records", "limit", 4, "ReadRecords with LIMIT 7",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of(), List.of(), List.of(), 7)));
        add("read_records", "order_by_limit", 4, "ReadRecords with ORDER BY int_col DESC NULLS LAST, id LIMIT 5",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of(), List.of(),
                        List.of(new OrderByField("int_col", OrderByField.Direction.DESC_NULLS_LAST),
                                new OrderByField("id", OrderByField.Direction.ASC_NULLS_FIRST)), 5)));
        add("read_records", "order_by_no_limit", 4, "ReadRecords with ORDER BY text_col ASC NULLS FIRST on one split",
                q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "text_col"), split(q, "*", clauses.get(4)),
                        constraints(Map.of(), List.of(), List.of(new OrderByField("text_col", OrderByField.Direction.ASC_NULLS_FIRST)),
                                Constraints.DEFAULT_NO_LIMIT)));
        add("read_records", "limit_with_range", 4, "ReadRecords with a range constraint, ORDER BY and LIMIT",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("int_col",
                        SortedRangeSet.copyOf(int32, List.of(Range.greaterThan(allocator, int32, 1000)), false)), List.of(),
                        List.of(new OrderByField("int_col", OrderByField.Direction.ASC_NULLS_LAST)), 3)));
        add("read_records", "expr_arithmetic", 4, "ReadRecords with ($add(int_col, 10) > 15000)",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of(),
                        List.of(call(bool, StandardFunctions.GREATER_THAN_OPERATOR_FUNCTION_NAME,
                                call(int32, StandardFunctions.ADD_FUNCTION_NAME, var("int_col", int32), constant(int32, 10)),
                                constant(int32, 15000))),
                        List.of(), Constraints.DEFAULT_NO_LIMIT)));
        add("read_records", "expr_like", 4, "ReadRecords with text_col LIKE 'text_0001%'",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of(),
                        List.of(call(bool, StandardFunctions.LIKE_PATTERN_FUNCTION_NAME, var("text_col", utf8()), constant(utf8(), "text_0001%"))),
                        List.of(), Constraints.DEFAULT_NO_LIMIT)));
        add("read_records", "expr_or_not_null", 4, "ReadRecords with (int_col = 2 OR NOT (double_col IS NULL)) on one split",
                q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "int_col", "double_col"), split(q, "*", clauses.get(5)),
                        constraints(Map.of(),
                                List.of(call(bool, StandardFunctions.OR_FUNCTION_NAME,
                                        call(bool, StandardFunctions.EQUAL_OPERATOR_FUNCTION_NAME, var("int_col", int32), constant(int32, 2)),
                                        call(bool, StandardFunctions.NOT_FUNCTION_NAME,
                                                call(bool, StandardFunctions.IS_NULL_FUNCTION_NAME, var("double_col", float8))))),
                                List.of(), Constraints.DEFAULT_NO_LIMIT)));
        add("read_records", "expr_in", 4, "ReadRecords with varchar_col $in ('beta', 'epsilon') via an array constructor",
                q -> read(q, allTypes, Schemas.allTypes(), split(q, "*", clauses.get(6)), constraints(Map.of(),
                        List.of(call(bool, StandardFunctions.IN_PREDICATE_FUNCTION_NAME, var("varchar_col", utf8()),
                                call(new ArrowType.List(), StandardFunctions.ARRAY_CONSTRUCTOR_FUNCTION_NAME,
                                        constant(utf8(), "beta"), constant(utf8(), "epsilon")))),
                        List.of(), Constraints.DEFAULT_NO_LIMIT)));
        add("read_records", "expr_unsupported", 4, "ReadRecords with a function the parser does not know (error or ignored)",
                q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id"), split(q, "*", clauses.get(0)), constraints(Map.of(),
                        List.of(new FunctionCallExpression(bool, new com.amazonaws.athena.connector.lambda.domain.predicate.functions.FunctionName("no_such_function"),
                                List.of(var("int_col", int32)))),
                        List.of(), Constraints.DEFAULT_NO_LIMIT)));

        TableName sales = new TableName("parity", "sales");
        add("read_records", "partition_2023", 1, "ReadRecords on one partition of the partitioned table",
                q -> read(q, sales, Schemas.sales(), split(q, "parity", "sales_2023"), constraints(Map.of())));
        add("read_records", "partition_default_range", 1, "ReadRecords on the default partition with a range constraint",
                q -> read(q, sales, Schemas.sales(), split(q, "parity", "sales_other"), constraints(Map.of("amount",
                        SortedRangeSet.copyOf(new ArrowType.Decimal(12, 2, 128), List.of(Range.greaterThan(allocator,
                                new ArrowType.Decimal(12, 2, 128), new BigDecimal("1000.00"))), false)))));
        add("read_records", "partition_projected", 1, "ReadRecords on a partition with partition columns in the schema",
                q -> read(q, sales, Schemas.salesWithPartitionCols(), split(q, "parity", "sales_2024"), constraints(Map.of("region",
                        SortedRangeSet.copyOf(utf8(), List.of(Range.equal(allocator, utf8(), "north")), false)))));
        add("read_records", "view", 1, "ReadRecords on a view",
                q -> read(q, new TableName("parity", "all_types_view"), Schemas.view(), split(q, "*", "*"), constraints(Map.of())));
        add("read_records", "materialized_view", 1, "ReadRecords on a materialized view",
                q -> read(q, new TableName("parity", "sales_summary_mv"), Schemas.matView(), split(q, "*", "*"), constraints(Map.of())));
        add("read_records", "camel_table", 1, "ReadRecords on a mixed-case table name with mixed-case columns",
                q -> read(q, new TableName("parity", "CamelTable"), Schemas.camelTable(), split(q, "*", "(\"Id\" >= 1 AND \"Id\" <= 15)"),
                        constraints(Map.of("CamelValue", SortedRangeSet.copyOf(utf8(), List.of(Range.greaterThan(allocator, utf8(), "Camel2")), false)))));
        add("read_records", "mixed_schema", 1, "ReadRecords on a mixed-case schema and table",
                q -> read(q, new TableName("MixedSchema", "MixedTable"), Schemas.mixedTable(), split(q, "*", "*"), constraints(Map.of())));
        add("read_records", "empty_table", 1, "ReadRecords on an empty table",
                q -> read(q, new TableName("parity", "empty_table"), Schemas.emptyTable(), split(q, "*", "*"), constraints(Map.of())));
        add("read_records", "uuid_pk", 1, "ReadRecords on a table with a uuid key read as varchar",
                q -> read(q, new TableName("parity", "uuid_pk"), Schemas.uuidPk(), split(q, "*", "*"), constraints(Map.of())));
        add("read_records", "unknown_table", 1, "ReadRecords on a missing table (error)",
                q -> read(q, new TableName("parity", "no_such_table"), Schemas.smallPk(), split(q, "*", "*"), constraints(Map.of())));
        add("read_records", "unknown_column", 1, "ReadRecords with a column the table does not have",
                q -> read(q, new TableName("parity", "small_pk"), SchemaBuilder.newBuilder().addField("id", int32).addField("ghost", utf8()).build(),
                        split(q, "*", "*"), constraints(Map.of())));
        add("read_records", "spill_multi_block", 1, "ReadRecords with a small block size so the result spills as several blocks",
                q -> read(q, CATALOG, allTypes, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of()), 200_000L, 0L));
        add("read_records", "spill_inline_threshold", 1, "ReadRecords whose block passes the inline limit and spills once",
                q -> read(q, CATALOG, allTypes, Schemas.project(Schemas.allTypes(), "id", "int_col"), split(q, "*", clauses.get(0)),
                        constraints(Map.of()), BIG_BLOCK, 100L));
        add("read_records", "spill_multi_block_encrypted", 1, "ReadRecords that spills several blocks with a random spill encryption key in the split",
                q -> read(q, CATALOG, allTypes, Schemas.allTypes(), encryptedSplit(q, "*", "*"), constraints(Map.of()), 200_000L, 0L));
        add("read_records", "spill_inline_threshold_encrypted", 1, "ReadRecords that spills one block with a random spill encryption key in the split",
                q -> read(q, CATALOG, allTypes, Schemas.project(Schemas.allTypes(), "id", "int_col"), encryptedSplit(q, "*", clauses.get(0)),
                        constraints(Map.of()), BIG_BLOCK, 100L));
        add("read_records", "inline_encrypted", 1, "ReadRecords with a random spill encryption key in the split and a result small enough to stay inline",
                q -> read(q, CATALOG, new TableName("parity", "small_pk"), Schemas.smallPk(), encryptedSplit(q, "*", "*"), constraints(Map.of()),
                        BIG_BLOCK, BIG_BLOCK));
        add("read_records", "secret_credentials", 1, "single", Map.of("default", SECRET_CONNECTION),
                "ReadRecords with credentials from the Secrets Manager stub",
                q -> read(q, new TableName("parity", "small_pk"), Schemas.smallPk(), split(q, "*", "*"), constraints(Map.of())));
        add("read_records", "mux_named", 1, "mux", Map.of(), "ReadRecords through a named mux catalog",
                q -> read(q, MUX_CATALOG, new TableName("parity", "small_pk"), Schemas.smallPk(), split(q, "*", "*"), constraints(Map.of()),
                        BIG_BLOCK, BIG_BLOCK));
        add("read_records", "mux_unknown_catalog", 1, "mux", Map.of(), "ReadRecords through an unknown mux catalog (error)",
                q -> read(q, "not_registered", new TableName("parity", "small_pk"), Schemas.smallPk(), split(q, "*", "*"), constraints(Map.of()),
                        BIG_BLOCK, BIG_BLOCK));

        String qptQuery = "SELECT id, smallint_col, bigint_col, numeric_scaled, real_col, double_col, text_col, char_col, bool_col, date_col, ts_col, bytea_col FROM parity.all_types WHERE id <= 40 ORDER BY id";
        add("read_records", "qpt", 5, "ReadRecords for query passthrough",
                q -> read(q, new TableName("system", "query"), Schemas.qptScalar(),
                        Split.newBuilder(spill(q, "split-0001"), null).applyProperties(qptArgs(qptQuery)).build(),
                        qptConstraints(qptArgs(qptQuery))));
        add("read_records", "qpt_bad_signature", 5, "ReadRecords for query passthrough with a wrong signature (error)",
                q -> read(q, new TableName("system", "query"), Schemas.qptScalar(),
                        Split.newBuilder(spill(q, "split-0001"), null).build(),
                        qptConstraints(Map.of("schemaFunctionName", "SYSTEM.OTHER", "QUERY", qptQuery))));
        String exportQuery = "SELECT id, numeric_unbounded, text_col, tstz_col, int_arr, uuid_col, time_col FROM parity.all_types WHERE id <= 25 ORDER BY id";
        add("read_records", "qpt_export", 5, "ReadRecords for query passthrough with the ExportSchema marker, writes to the S3 stub",
                q -> {
                    Map<String, String> args = new LinkedHashMap<>(qptArgs(exportQuery));
                    args.put("ExportSchema", "true");
                    return read(q, CATALOG, new TableName("system", "query"), SchemaBuilder.newBuilder().build(),
                            Split.newBuilder(spill(q, "split-0001"), null).applyProperties(args).build(),
                            qptConstraints(args), 1_000_000L, 0L);
                });

        String plain = substraitPlan("SELECT id, int_col, text_col FROM all_types WHERE int_col > -4000 AND id < 300");
        String ordered = substraitPlan("SELECT id, double_col FROM all_types WHERE double_col IS NOT NULL ORDER BY double_col DESC LIMIT 6");
        String inList = substraitPlan("SELECT id, varchar_col, bool_col FROM all_types WHERE varchar_col IN ('alpha', 'delta') AND bool_col = TRUE AND id <= 60");
        add("read_records", "substrait_filter", 6, "ReadRecords with a Substrait plan carrying a filter",
                q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "int_col", "text_col"), split(q, "*", "*"),
                        planConstraints(plain)));
        add("read_records", "substrait_order_limit", 6, "ReadRecords with a Substrait plan carrying ORDER BY and LIMIT",
                q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "double_col"), split(q, "*", "*"),
                        planConstraints(ordered)));
        add("read_records", "substrait_split", 6, "ReadRecords with a Substrait plan on a primary key range split",
                q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "int_col", "text_col"), split(q, "*", clauses.get(0)),
                        planConstraints(plain)));
        add("read_records", "substrait_in_list", 6, "ReadRecords with a Substrait plan carrying IN and boolean filters",
                q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "varchar_col", "bool_col"), split(q, "*", "*"),
                        planConstraints(inList)));
        add("read_records", "substrait_casing_filter", 6, "ReadRecords with a Substrait plan and CATALOG_CASING_FILTER in the split",
                q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "int_col", "text_col"),
                        Split.newBuilder(spill(q, "split-0001"), null).add("partition_schema_name", "*").add("partition_name", "*")
                                .add("CATALOG_CASING_FILTER", "UPPERCASE_ONLY").build(),
                        planConstraints(plain)));
        add("read_records", "substrait_invalid_plan", 6, "ReadRecords with a Substrait plan that is not valid base64 protobuf (error)",
                q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id"), split(q, "*", "*"),
                        planConstraints(Base64.getEncoder().encodeToString("not a plan".getBytes(StandardCharsets.UTF_8)))));
    }

    private static Constraints planConstraints(String plan)
    {
        return new Constraints(Map.of(), List.of(), List.of(), Constraints.DEFAULT_NO_LIMIT, Map.of(), new QueryPlan("0.65.0", plan));
    }

    private static FunctionCallExpression call(ArrowType type, StandardFunctions function, FederationExpression... args)
    {
        return new FunctionCallExpression(type, function.getFunctionName(), Arrays.asList(args));
    }

    private static VariableExpression var(String name, ArrowType type)
    {
        return new VariableExpression(name, type);
    }

    private ConstantExpression constant(ArrowType type, Object value)
    {
        return new ConstantExpression(BlockUtils.newBlock(allocator, "col1", type, List.of(value)), type);
    }

    private static RelDataType nullable(RelDataTypeFactory factory, RelDataType type)
    {
        return factory.createTypeWithNullability(type, true);
    }

    static String substraitPlan(String sql) throws Exception
    {
        Table table = new AbstractTable()
        {
            @Override
            public RelDataType getRowType(RelDataTypeFactory factory)
            {
                return factory.createStructType(Arrays.asList(
                        Pair.of("id", factory.createTypeWithNullability(factory.createSqlType(SqlTypeName.INTEGER), false)),
                        Pair.of("smallint_col", nullable(factory, factory.createSqlType(SqlTypeName.SMALLINT))),
                        Pair.of("int_col", nullable(factory, factory.createSqlType(SqlTypeName.INTEGER))),
                        Pair.of("bigint_col", nullable(factory, factory.createSqlType(SqlTypeName.BIGINT))),
                        Pair.of("numeric_scaled", nullable(factory, factory.createSqlType(SqlTypeName.DECIMAL, 18, 4))),
                        Pair.of("real_col", nullable(factory, factory.createSqlType(SqlTypeName.REAL))),
                        Pair.of("double_col", nullable(factory, factory.createSqlType(SqlTypeName.DOUBLE))),
                        Pair.of("text_col", nullable(factory, factory.createSqlType(SqlTypeName.VARCHAR))),
                        Pair.of("varchar_col", nullable(factory, factory.createSqlType(SqlTypeName.VARCHAR))),
                        Pair.of("bool_col", nullable(factory, factory.createSqlType(SqlTypeName.BOOLEAN))),
                        Pair.of("date_col", nullable(factory, factory.createSqlType(SqlTypeName.DATE)))));
            }
        };
        AbstractSchema schema = new AbstractSchema()
        {
            @Override
            protected Map<String, Table> getTableMap()
            {
                return Map.of("all_types", table);
            }
        };
        CalciteSchema root = CalciteSchema.createRootSchema(false);
        root.add("parity", schema);
        CalciteCatalogReader catalog = new CalciteCatalogReader(root, List.of("parity"),
                new SqlTypeFactoryImpl(AnsiSqlDialect.DEFAULT.getTypeSystem()),
                CalciteConnectionConfig.DEFAULT.set(CalciteConnectionProperty.CASE_SENSITIVE, Boolean.FALSE.toString()));
        io.substrait.plan.Plan plan = new SqlToSubstrait().convert(sql, catalog);
        return Base64.getEncoder().encodeToString(new PlanProtoConverter().toProto(plan).toByteArray());
    }

    private void defineExtra() throws Exception
    {
        TableName edges = new TableName("parity", "edges");
        TableName big = new TableName("parity", "big");
        TableName allTypes = new TableName("parity", "all_types");
        ArrowType int32 = Types.MinorType.INT.getType();
        ArrowType int64 = Types.MinorType.BIGINT.getType();
        ArrowType float8 = Types.MinorType.FLOAT8.getType();
        ArrowType float4 = Types.MinorType.FLOAT4.getType();
        ArrowType int16 = Types.MinorType.SMALLINT.getType();
        ArrowType bool = Types.MinorType.BIT.getType();
        ArrowType dateDay = Types.MinorType.DATEDAY.getType();
        ArrowType dateMilli = Types.MinorType.DATEMILLI.getType();
        ArrowType dec184 = new ArrowType.Decimal(18, 4, 128);
        ArrowType dec380 = new ArrowType.Decimal(38, 0, 128);
        int[] ids = {1, 2, 3, 4, 5, 6, 7, 8, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 40, 41, 42, 60, 61, 62, 63, 64, 65, 80};
        for (int id : ids) {
            add("x_edge", "row_" + id, 6, "edges row " + id,
                    q -> read(q, edges, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of("id",
                            SortedRangeSet.copyOf(int32, List.of(Range.equal(allocator, int32, id)), false)))));
        }
        add("x_edge", "get_table", 6, "GetTable edges", q -> new GetTableRequest(identity(), q, CATALOG, edges, Map.of()));
        add("x_edge", "get_table_odd", 6, "GetTable odd_types", q -> new GetTableRequest(identity(), q, CATALOG, new TableName("parity", "odd_types"), Map.of()));
        Schema odd = SchemaBuilder.newBuilder().addField("id", int32).addField("m", float8).addField("iv", utf8()).addField("n52", new ArrowType.Decimal(5, 2, 128))
                .addField("ts3", dateMilli).addField("tstz0", dateMilli).addField("vc", utf8()).addField("ch", utf8()).addListField("i2arr", int16)
                .addListField("f4", float4).addField("ja", utf8()).addField("ip", utf8()).addField("bits", utf8()).addField("xm", utf8()).addField("en", utf8())
                .addField("partition_schema_name", utf8()).addField("partition_name", utf8()).build();
        add("x_edge", "odd_full", 6, "odd types full read", q -> read(q, new TableName("parity", "odd_types"), odd, split(q, "*", "*"), constraints(Map.of())));
        for (String col : new String[]{"m", "iv", "n52", "ts3", "tstz0", "i2arr", "f4", "ja", "ip", "bits", "xm"}) {
            add("x_edge", "odd_col_" + col, 6, "odd types column " + col,
                    q -> read(q, new TableName("parity", "odd_types"), Schemas.project(odd, "id", col), split(q, "*", "*"), constraints(Map.of())));
        }
        add("x_pred", "bigint_min_eq", 6, "bigint = Long.MIN", q -> read(q, edges, Schemas.project(Schemas.allTypes(), "id", "bigint_col"), split(q, "*", "*"),
                constraints(Map.of("bigint_col", SortedRangeSet.copyOf(int64, List.of(Range.equal(allocator, int64, Long.MIN_VALUE)), false)))));
        add("x_pred", "bigint_extremes_in", 6, "bigint in (min,max)", q -> read(q, edges, Schemas.project(Schemas.allTypes(), "id", "bigint_col"), split(q, "*", "*"),
                constraints(Map.of("bigint_col", SortedRangeSet.copyOf(int64, List.of(Range.equal(allocator, int64, Long.MIN_VALUE), Range.equal(allocator, int64, Long.MAX_VALUE)), true)))));
        add("x_pred", "int_not_equal", 6, "int_col <> 2 as two ranges", q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "int_col"), split(q, "*", "*"),
                constraints(Map.of("int_col", SortedRangeSet.copyOf(int32, List.of(Range.lessThan(allocator, int32, 2), Range.greaterThan(allocator, int32, 2)), false)))));
        add("x_pred", "int_mixed_in_range", 6, "int in (9,16) or between 100 and 120 or null", q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "int_col"), split(q, "*", "*"),
                constraints(Map.of("int_col", SortedRangeSet.copyOf(int32, List.of(Range.equal(allocator, int32, 9), Range.equal(allocator, int32, 16), Range.range(allocator, int32, 100, true, 120, true)), true)))));
        add("x_pred", "double_nan_eq", 6, "double_col = NaN", q -> read(q, edges, Schemas.project(Schemas.allTypes(), "id", "double_col"), split(q, "*", "*"),
                constraints(Map.of("double_col", SortedRangeSet.copyOf(float8, List.of(Range.equal(allocator, float8, Double.NaN)), false)))));
        add("x_pred", "double_gt_inf_range", 6, "double_col > 1e300", q -> read(q, edges, Schemas.project(Schemas.allTypes(), "id", "double_col"), split(q, "*", "*"),
                constraints(Map.of("double_col", SortedRangeSet.copyOf(float8, List.of(Range.greaterThan(allocator, float8, 1e300)), false)))));
        add("x_pred", "real_small", 6, "real_col < 1e-40", q -> read(q, edges, Schemas.project(Schemas.allTypes(), "id", "real_col"), split(q, "*", "*"),
                constraints(Map.of("real_col", SortedRangeSet.copyOf(float4, List.of(Range.range(allocator, float4, 0.0F, false, 1e-40F, true)), false)))));
        add("x_pred", "varchar_quote", 6, "varchar = O'Brien", q -> read(q, edges, Schemas.project(Schemas.allTypes(), "id", "varchar_col", "text_col"), split(q, "*", "*"),
                constraints(Map.of("varchar_col", EquatableValueSet.newBuilder(allocator, utf8(), true, false).add("O'Brien").build()))));
        add("x_pred", "text_backslash_range", 6, "text_col range with backslash", q -> read(q, edges, Schemas.project(Schemas.allTypes(), "id", "text_col"), split(q, "*", "*"),
                constraints(Map.of("text_col", SortedRangeSet.copyOf(utf8(), List.of(Range.range(allocator, utf8(), "it's \\", true, "it's \\ \"r", false)), false)))));
        add("x_pred", "text_empty_eq", 6, "text_col = ''", q -> read(q, edges, Schemas.project(Schemas.allTypes(), "id", "text_col"), split(q, "*", "*"),
                constraints(Map.of("text_col", SortedRangeSet.copyOf(utf8(), List.of(Range.equal(allocator, utf8(), "")), false)))));
        add("x_pred", "ts_eq_millis", 6, "ts_col = 2020-06-01 12:00:00.123", q -> read(q, edges, Schemas.project(Schemas.allTypes(), "id", "ts_col"), split(q, "*", "*"),
                constraints(Map.of("ts_col", SortedRangeSet.copyOf(dateMilli, List.of(Range.equal(allocator, dateMilli, LocalDateTime.of(2020, 6, 1, 12, 0, 0, 123_000_000))), false)))));
        add("x_pred", "ts_old_range", 6, "ts_col < 1500", q -> read(q, edges, Schemas.project(Schemas.allTypes(), "id", "ts_col"), split(q, "*", "*"),
                constraints(Map.of("ts_col", SortedRangeSet.copyOf(dateMilli, List.of(Range.lessThan(allocator, dateMilli, LocalDateTime.of(1500, 1, 1, 0, 0))), false)))));
        add("x_pred", "tstz_lt", 6, "tstz_col <= 1970", q -> read(q, edges, Schemas.project(Schemas.allTypes(), "id", "tstz_col"), split(q, "*", "*"),
                constraints(Map.of("tstz_col", SortedRangeSet.copyOf(dateMilli, List.of(Range.lessThanOrEqual(allocator, dateMilli, LocalDateTime.of(1970, 1, 1, 0, 0))), false)))));
        add("x_pred", "date_multi_null", 6, "dates in two ranges or null", q -> read(q, edges, Schemas.project(Schemas.allTypes(), "id", "date_col"), split(q, "*", "*"),
                constraints(Map.of("date_col", SortedRangeSet.copyOf(dateDay, List.of(Range.lessThan(allocator, dateDay, (int) LocalDate.of(1600, 1, 1).toEpochDay()),
                        Range.range(allocator, dateDay, (int) LocalDate.of(1969, 12, 31).toEpochDay(), true, (int) LocalDate.of(2020, 6, 1).toEpochDay(), true)), true)))));
        add("x_pred", "date_in_list", 6, "date in list", q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "date_col"), split(q, "*", "*"),
                constraints(Map.of("date_col", SortedRangeSet.copyOf(dateDay, List.of(Range.equal(allocator, dateDay, (int) LocalDate.of(1960, 1, 12).toEpochDay()),
                        Range.equal(allocator, dateDay, (int) LocalDate.of(1960, 1, 23).toEpochDay()), Range.equal(allocator, dateDay, (int) LocalDate.of(1960, 2, 3).toEpochDay())), false)))));
        add("x_pred", "decimal_eq", 6, "numeric_scaled = 12.3457", q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "numeric_scaled"), split(q, "*", "*"),
                constraints(Map.of("numeric_scaled", SortedRangeSet.copyOf(dec184, List.of(Range.equal(allocator, dec184, new BigDecimal("12.3457"))), false)))));
        add("x_pred", "decimal38_gt", 6, "numeric_unbounded > 10^37", q -> read(q, edges, Schemas.project(Schemas.allTypes(), "id", "numeric_unbounded"), split(q, "*", "*"),
                constraints(Map.of("numeric_unbounded", SortedRangeSet.copyOf(dec380, List.of(Range.greaterThan(allocator, dec380, new BigDecimal("10000000000000000000000000000000000000"))), false)))));
        add("x_pred", "smallint_in", 6, "smallint in extremes", q -> read(q, edges, Schemas.project(Schemas.allTypes(), "id", "smallint_col"), split(q, "*", "*"),
                constraints(Map.of("smallint_col", SortedRangeSet.copyOf(int16, List.of(Range.equal(allocator, int16, (short) -32768), Range.equal(allocator, int16, (short) 32767)), false)))));
        add("x_pred", "equatable_deny_null", 6, "varchar not in (alpha,beta) with null allowed", q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "varchar_col"), split(q, "*", "*"),
                constraints(Map.of("varchar_col", EquatableValueSet.newBuilder(allocator, utf8(), false, true).add("alpha").add("beta").build()))));
        add("x_pred", "bool_false_or_null", 6, "bool false or null", q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "bool_col"), split(q, "*", "*"),
                constraints(Map.of("bool_col", SortedRangeSet.copyOf(bool, List.of(Range.equal(allocator, bool, false)), true)))));
        add("x_pred", "limit_zero", 6, "limit 0", q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id"), split(q, "*", "*"),
                constraints(Map.of(), List.of(), List.of(), 0)));
        add("x_pred", "order_multi", 6, "order by three columns", q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "bool_col", "varchar_col", "date_col"), split(q, "*", "*"),
                constraints(Map.of(), List.of(), List.of(new OrderByField("bool_col", OrderByField.Direction.DESC_NULLS_FIRST),
                        new OrderByField("varchar_col", OrderByField.Direction.ASC_NULLS_LAST), new OrderByField("date_col", OrderByField.Direction.DESC_NULLS_LAST)), 25)));
        add("x_pred", "expr_neq_mod", 6, "int_col % 7 <> 0", q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "int_col"), split(q, "*", "*"),
                constraints(Map.of(), List.of(call(bool, StandardFunctions.NOT_EQUAL_OPERATOR_FUNCTION_NAME,
                        call(int32, StandardFunctions.MODULUS_FUNCTION_NAME, var("int_col", int32), constant(int32, 7)), constant(int32, 0))), List.of(), 40)));
        add("x_pred", "expr_negate_lte", 6, "-int_col <= -5000", q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "int_col"), split(q, "*", "*"),
                constraints(Map.of(), List.of(call(bool, StandardFunctions.LESS_THAN_OR_EQUAL_OPERATOR_FUNCTION_NAME,
                        call(int32, StandardFunctions.NEGATE_FUNCTION_NAME, var("int_col", int32)), constant(int32, -5000))), List.of(), -1)));
        add("x_pred", "expr_distinct", 6, "varchar is distinct from alpha", q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "varchar_col"), split(q, "*", "*"),
                constraints(Map.of(), List.of(call(bool, StandardFunctions.IS_DISTINCT_FROM_OPERATOR_FUNCTION_NAME, var("varchar_col", utf8()), constant(utf8(), "alpha"))), List.of(), 30)));
        add("x_pred", "expr_like_quote", 6, "text like with quote", q -> read(q, edges, Schemas.project(Schemas.allTypes(), "id", "text_col"), split(q, "*", "*"),
                constraints(Map.of(), List.of(call(bool, StandardFunctions.LIKE_PATTERN_FUNCTION_NAME, var("text_col", utf8()), constant(utf8(), "it's%"))), List.of(), -1)));
        add("x_pred", "expr_and_mul_div", 6, "and of mul and div", q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "int_col", "bigint_col"), split(q, "*", "*"),
                constraints(Map.of(), List.of(call(bool, StandardFunctions.AND_FUNCTION_NAME,
                        call(bool, StandardFunctions.GREATER_THAN_OR_EQUAL_OPERATOR_FUNCTION_NAME, call(int32, StandardFunctions.MULTIPLY_FUNCTION_NAME, var("int_col", int32), constant(int32, 3)), constant(int32, 900)),
                        call(bool, StandardFunctions.LESS_THAN_OPERATOR_FUNCTION_NAME, call(int32, StandardFunctions.DIVIDE_FUNCTION_NAME, var("int_col", int32), constant(int32, 2)), constant(int32, 600)))), List.of(), -1)));
        add("x_pred", "expr_nullif", 6, "nullif", q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), "id", "int_col"), split(q, "*", "*"),
                constraints(Map.of(), List.of(call(bool, StandardFunctions.IS_NULL_FUNCTION_NAME,
                        call(int32, StandardFunctions.NULLIF_FUNCTION_NAME, var("int_col", int32), constant(int32, 2))) ), List.of(), 20)));
        String[] plans = {
            "SELECT id, text_col FROM all_types WHERE text_col LIKE 'text_0001%' AND id < 2000",
            "SELECT id, int_col FROM all_types WHERE NOT (int_col BETWEEN -4000 AND 15000)",
            "SELECT id, bigint_col, double_col FROM all_types WHERE bigint_col IS NULL OR double_col < 0",
            "SELECT id, date_col FROM all_types WHERE date_col >= DATE '1990-01-01' ORDER BY date_col LIMIT 10",
            "SELECT id, ts_col FROM all_types WHERE ts_col < TIMESTAMP '1966-01-01 00:00:00'",
            "SELECT id, int_col FROM all_types ORDER BY int_col NULLS FIRST, id DESC LIMIT 7",
            "SELECT id, varchar_col FROM all_types WHERE varchar_col <> 'beta' AND varchar_col NOT IN ('alpha', 'gamma') AND id < 100",
            "SELECT id, numeric_scaled FROM all_types WHERE numeric_scaled > 100.5 AND numeric_scaled <= 1000 ORDER BY numeric_scaled DESC LIMIT 5",
            "SELECT id, text_col FROM all_types WHERE text_col = 'it''s' OR id = 3",
            "SELECT id, real_col FROM all_types WHERE real_col BETWEEN 10 AND 20 OR real_col IS NULL",
            "SELECT id, int_col FROM all_types WHERE int_col + 1 > 100 AND int_col * 2 < 1000 AND int_col - 3 <> 50",
        };
        for (int i = 0; i < plans.length; i++) {
            String sql = plans[i];
            String plan;
            try {
                plan = substraitPlan(sql);
            }
            catch (Exception e) {
                System.err.println("plan failed: " + sql + ": " + e);
                continue;
            }
            String[] cols = sql.substring(7, sql.indexOf(" FROM")).split(", ");
            add("x_substrait", "plan_" + i, 6, sql, q -> read(q, allTypes, Schemas.project(Schemas.allTypes(), cols), split(q, "*", "*"), planConstraints(plan)));
        }
        add("x_big", "full_spill", 6, "200k rows all types, 1MB blocks", q -> read(q, CATALOG, big, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of()), 1_000_000L, 0L));
        add("x_big", "full_spill_encrypted", 6, "200k rows encrypted spill", q -> read(q, CATALOG, big, Schemas.project(Schemas.allTypes(), "id", "text_col", "numeric_unbounded", "int_arr", "tstz_col"), encryptedSplit(q, "*", "*"), constraints(Map.of()), 4_000_000L, 0L));
        add("x_big", "inline_large", 6, "big inline response", q -> read(q, big, Schemas.project(Schemas.allTypes(), "id", "int_col", "date_col"), split(q, "*", "*"),
                constraints(Map.of("id", SortedRangeSet.copyOf(int32, List.of(Range.lessThanOrEqual(allocator, int32, 60000)), false)))));
        add("x_big", "edges_full", 6, "edges full", q -> read(q, edges, Schemas.allTypes(), split(q, "*", "*"), constraints(Map.of())));
    }

}
