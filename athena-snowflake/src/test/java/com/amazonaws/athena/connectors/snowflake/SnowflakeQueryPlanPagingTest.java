/*-
 * #%L
 * athena-snowflake
 * %%
 * Copyright (C) 2019 - 2026 Amazon Web Services
 * %%
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 * #L%
 */
package com.amazonaws.athena.connectors.snowflake;

import com.amazonaws.athena.connector.lambda.data.SchemaBuilder;
import com.amazonaws.athena.connector.lambda.domain.Split;
import com.amazonaws.athena.connector.lambda.domain.predicate.Constraints;
import com.amazonaws.athena.connector.lambda.domain.predicate.QueryPlan;
import com.amazonaws.athena.connector.lambda.domain.spill.S3SpillLocation;
import org.apache.arrow.vector.types.pojo.Schema;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.mockito.ArgumentCaptor;

import java.sql.Connection;
import java.sql.DriverManager;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.sql.SQLException;
import java.sql.Statement;
import java.util.ArrayList;
import java.util.Collections;
import java.util.Comparator;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import static com.amazonaws.athena.connector.lambda.domain.predicate.Constraints.DEFAULT_NO_LIMIT;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.mockito.ArgumentMatchers.anyString;
import static org.mockito.Mockito.mock;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

class SnowflakeQueryPlanPagingTest
{
    private static final Map<String, String> PLANS = new LinkedHashMap<>();
    private static final int TOP_N_LIMIT = 7;
    private static final String TOP_N = "SELECT id, name FROM test_table ORDER BY name DESC LIMIT 7";
    private static final int[] ROW_COUNTS = {1, 7, 23};
    private static final int[] PAGE_SIZES = {1, 3, 7, 23, 50};

    static {
        PLANS.put("SELECT id, name FROM test_table",
                "Gm0SawpfOl0KBhIECgICAxI9CjsKAgoAEhoKAmlkCgRuYW1lEg4KBCoCEAIKBGICEAEYAjoZCgt0ZXN0X3NjaGVtYQoKdGVzdF90YWJsZRoIEgYKAhIAIgAaChIICgQSAggBIgASAklEEgROQU1FMgsQSioHaXN0aG11cw==");
        PLANS.put("SELECT id, name FROM test_table WHERE id > 10",
                "Ch4IARIaL2Z1bmN0aW9uc19jb21wYXJpc29uLnlhbWwSEhoQCAEQARoKZ3Q6YW55X2FueRqYARKVAQqIATqFAQoGEgQKAgIDEmUSYwoCCgASPQo7CgIKABIaCgJpZAoEbmFtZRIOCgQqAhACCgRiAhABGAI6GQoLdGVzdF9zY2hlbWEKCnRlc3RfdGFibGUaHhocCAEaBAoCEAIiChoIEgYKAhIAIgAiBhoECgIoChoIEgYKAhIAIgAaChIICgQSAggBIgASAklEEgROQU1FMgsQSioHaXN0aG11cw==");
        PLANS.put("SELECT id, name FROM test_table WHERE id < 5 OR name = 'n17'",
                "ChsIARIXL2Z1bmN0aW9uc19ib29sZWFuLnlhbWwKHggCEhovZnVuY3Rpb25zX2NvbXBhcmlzb24ueWFtbBIPGg0IARABGgdvcjpib29sEhIaEAgCEAIaCmx0OmFueV9hbnkSFRoTCAIQAxoNZXF1YWw6YW55X2FueRrPARLMAQq/ATq8AQoGEgQKAgIDEpsBEpgBCgIKABI9CjsKAgoAEhoKAmlkCgRuYW1lEg4KBCoCEAIKBGICEAEYAjoZCgt0ZXN0X3NjaGVtYQoKdGVzdF90YWJsZRpTGlEIARoECgIQASIgGh4aHAgCGgQKAhACIgoaCBIGCgISACIAIgYaBAoCKAUiJRojGiEIAxoECgIQASIMGgoSCAoEEgIIASIAIgkaBwoFYgNuMTcaCBIGCgISACIAGgoSCAoEEgIIASIAEgJJRBIETkFNRTILEEoqB2lzdGhtdXM=");
        PLANS.put(TOP_N,
                "GpQBEpEBCoQBGoEBCgIKABJ3KnUKAgoAEl86XQoGEgQKAgIDEj0KOwoCCgASGgoCaWQKBG5hbWUSDgoEKgIQAgoEYgIQARgCOhkKC3Rlc3Rfc2NoZW1hCgp0ZXN0X3RhYmxlGggSBgoCEgAiABoKEggKBBICCAEiABoOCgoSCAoEEgIIASIAEAMYACAHEgJJRBIETkFNRTILEEoqB2lzdGhtdXM=");
        PLANS.put("SELECT name FROM test_table WHERE id >= 3",
                "Ch4IARIaL2Z1bmN0aW9uc19jb21wYXJpc29uLnlhbWwSExoRCAEQARoLZ3RlOmFueV9hbnkahwEShAEKfDp6CgUSAwoBAhJlEmMKAgoAEj0KOwoCCgASGgoCaWQKBG5hbWUSDgoEKgIQAgoEYgIQARgCOhkKC3Rlc3Rfc2NoZW1hCgp0ZXN0X3RhYmxlGh4aHAgBGgQKAhACIgoaCBIGCgISACIAIgYaBAoCKAMaChIICgQSAggBIgASBE5BTUUyCxBKKgdpc3RobXVz");
    }

    private final SnowflakeQueryStringBuilder queryBuilder = new SnowflakeQueryStringBuilder("\"", null);
    private final Schema schema = SchemaBuilder.newBuilder().addIntField("id").addStringField("name").build();
    private Connection connection;

    @BeforeEach
    void setUp() throws SQLException
    {
        connection = DriverManager.getConnection("jdbc:h2:mem:snowflake_paging");
    }

    @AfterEach
    void tearDown() throws SQLException
    {
        connection.close();
    }

    @Test
    void testPagedSplitsReturnEveryRowOnceOnQueryPlanPath() throws SQLException
    {
        int configurations = 0;
        for (int rowCount : ROW_COUNTS) {
            loadTable(rowCount);
            for (Map.Entry<String, String> plan : PLANS.entrySet()) {
                List<String> expected = run(plan.getValue(), unpagedSplit());
                for (int pageSize : PAGE_SIZES) {
                    List<String> union = new ArrayList<>();
                    for (int offset = 0; offset < rowCount; offset += pageSize) {
                        union.addAll(run(plan.getValue(), pagedSplit(pageSize, offset)));
                    }
                    String config = plan.getKey() + " rows=" + rowCount + " page=" + pageSize;
                    assertEquals(union.size(), new HashSet<>(union).size(), "duplicate rows for " + config + ": " + union);
                    assertEquals(sorted(expected, plan.getKey()), sorted(union, plan.getKey()), "rows for " + config);
                    configurations++;
                }
            }
        }
        assertEquals(ROW_COUNTS.length * PLANS.size() * PAGE_SIZES.length, configurations);
    }

    @Test
    void testPagedSplitSqlOnQueryPlanPath() throws SQLException
    {
        String sql = sql(PLANS.get("SELECT id, name FROM test_table WHERE id < 5 OR name = 'n17'"), pagedSplit(2, 4));
        assertEquals("SELECT *\nFROM (SELECT * FROM \"test_schema\".\"test_table\" ORDER BY \"id\" LIMIT 2 OFFSET 4) AS \"test_table\"\n"
                + "WHERE \"id\" < ? OR \"name\" = ?", sql);
    }

    @Test
    void testPagedSplitRejectsQueryPlanWithoutTable()
    {
        // VALUES (1, 'a'), (2, 'b')
        String plan = "GlkSVwpFCkMKAgoAEiMKBkVYUFIkMAoGRVhQUiQxEhEKBCoCEAIKB6oBBAgBGAIYAioYCgoKAigBCgSqAQFhCgoKAigCCgSqAQFiEgZFWFBSJDASBkVYUFIkMTILEEoqB2lzdGhtdXM=";
        RuntimeException error = assertThrows(RuntimeException.class, () -> sql(plan, pagedSplit(2, 0)));
        assertEquals("Failed to prepare statement with Calcite", error.getMessage());
        assertEquals("Cannot apply the split to a query plan that does not read one table: SELECT *\n"
                + "FROM (VALUES (1, 'a'),\n(2, 'b')) AS \"t\" (\"EXPR$0\", \"EXPR$1\")", error.getCause().getMessage());
    }

    private List<String> sorted(List<String> rows, String query)
    {
        List<String> copy = new ArrayList<>(rows);
        if (query.equals(TOP_N)) {
            copy.sort(Comparator.comparing((String row) -> row.substring(row.indexOf('|') + 1)).reversed());
            copy = new ArrayList<>(copy.subList(0, Math.min(TOP_N_LIMIT, copy.size())));
        }
        Collections.sort(copy);
        return copy;
    }

    private void loadTable(int rowCount) throws SQLException
    {
        try (Statement statement = connection.createStatement()) {
            statement.execute("DROP SCHEMA IF EXISTS \"test_schema\" CASCADE");
            statement.execute("CREATE SCHEMA \"test_schema\"");
            statement.execute("CREATE TABLE \"test_schema\".\"test_table\" (\"id\" INT PRIMARY KEY, \"name\" VARCHAR(16))");
            for (int id = rowCount; id >= 1; id--) {
                statement.execute("INSERT INTO \"test_schema\".\"test_table\" VALUES (" + id + ", 'n" + id + "')");
            }
        }
    }

    private List<String> run(String plan, Split split) throws SQLException
    {
        List<String> rows = new ArrayList<>();
        try (PreparedStatement statement = queryBuilder.buildSql(connection, null, "test_schema", "test_table", schema, constraints(plan), split);
                ResultSet resultSet = statement.executeQuery()) {
            int columns = resultSet.getMetaData().getColumnCount();
            while (resultSet.next()) {
                StringBuilder row = new StringBuilder();
                for (int column = 1; column <= columns; column++) {
                    row.append(column > 1 ? "|" : "").append(resultSet.getString(column));
                }
                rows.add(row.toString());
            }
        }
        return rows;
    }

    private String sql(String plan, Split split) throws SQLException
    {
        Connection mockConnection = mock(Connection.class);
        when(mockConnection.prepareStatement(anyString())).thenReturn(mock(PreparedStatement.class));
        queryBuilder.buildSql(mockConnection, null, "test_schema", "test_table", schema, constraints(plan), split);
        ArgumentCaptor<String> sql = ArgumentCaptor.forClass(String.class);
        verify(mockConnection).prepareStatement(sql.capture());
        return sql.getValue();
    }

    private static Constraints constraints(String plan)
    {
        return new Constraints(Collections.emptyMap(), Collections.emptyList(), Collections.emptyList(),
                DEFAULT_NO_LIMIT, Collections.emptyMap(), new QueryPlan("", plan));
    }

    private static Split unpagedSplit()
    {
        return Split.newBuilder(S3SpillLocation.newBuilder().withBucket("bucket").withPrefix("prefix").build(), null).build();
    }

    private static Split pagedSplit(int pageSize, int offset)
    {
        return Split.newBuilder(S3SpillLocation.newBuilder().withBucket("bucket").withPrefix("prefix").build(), null)
                .add(SnowflakeConstants.BLOCK_PARTITION_COLUMN_NAME, "partition-primary-\"id\"-limit-" + pageSize + "-offset-" + offset)
                .build();
    }
}
