/*-
 * #%L
 * athena-postgresql
 * %%
 * Copyright (C) 2026 Amazon Web Services
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
package com.amazonaws.athena.connectors.postgresql;

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
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import static com.amazonaws.athena.connector.lambda.domain.predicate.Constraints.DEFAULT_NO_LIMIT;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.mockito.ArgumentMatchers.anyString;
import static org.mockito.Mockito.mock;
import static org.mockito.Mockito.times;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

class PostGreSqlQueryPlanPartitionTest
{
    private static final Map<String, String> PLANS = new LinkedHashMap<>();
    private static final String[][] CHILDREN = {{"test_table_low", "1", "7"}, {"test_table_mid", "8", "15"}, {"test_table_high", "16", "23"}};
    private static final int ROW_COUNT = 23;

    static {
        PLANS.put("SELECT id, name FROM test_table",
                "Gm0SawpfOl0KBhIECgICAxI9CjsKAgoAEhoKAmlkCgRuYW1lEg4KBCoCEAIKBGICEAEYAjoZCgt0ZXN0X3NjaGVtYQoKdGVzdF90YWJsZRoIEgYKAhIAIgAaChIICgQSAggBIgASAklEEgROQU1FMgsQSioHaXN0aG11cw==");
        PLANS.put("SELECT id, name FROM test_table WHERE id > 10",
                "Ch4IARIaL2Z1bmN0aW9uc19jb21wYXJpc29uLnlhbWwSEhoQCAEQARoKZ3Q6YW55X2FueRqYARKVAQqIATqFAQoGEgQKAgIDEmUSYwoCCgASPQo7CgIKABIaCgJpZAoEbmFtZRIOCgQqAhACCgRiAhABGAI6GQoLdGVzdF9zY2hlbWEKCnRlc3RfdGFibGUaHhocCAEaBAoCEAIiChoIEgYKAhIAIgAiBhoECgIoChoIEgYKAhIAIgAaChIICgQSAggBIgASAklEEgROQU1FMgsQSioHaXN0aG11cw==");
        PLANS.put("SELECT id, name FROM test_table WHERE id < 5 OR name = 'n17'",
                "ChsIARIXL2Z1bmN0aW9uc19ib29sZWFuLnlhbWwKHggCEhovZnVuY3Rpb25zX2NvbXBhcmlzb24ueWFtbBIPGg0IARABGgdvcjpib29sEhIaEAgCEAIaCmx0OmFueV9hbnkSFRoTCAIQAxoNZXF1YWw6YW55X2FueRrPARLMAQq/ATq8AQoGEgQKAgIDEpsBEpgBCgIKABI9CjsKAgoAEhoKAmlkCgRuYW1lEg4KBCoCEAIKBGICEAEYAjoZCgt0ZXN0X3NjaGVtYQoKdGVzdF90YWJsZRpTGlEIARoECgIQASIgGh4aHAgCGgQKAhACIgoaCBIGCgISACIAIgYaBAoCKAUiJRojGiEIAxoECgIQASIMGgoSCAoEEgIIASIAIgkaBwoFYgNuMTcaCBIGCgISACIAGgoSCAoEEgIIASIAEgJJRBIETkFNRTILEEoqB2lzdGhtdXM=");
        PLANS.put("SELECT name FROM test_table WHERE id >= 3",
                "Ch4IARIaL2Z1bmN0aW9uc19jb21wYXJpc29uLnlhbWwSExoRCAEQARoLZ3RlOmFueV9hbnkahwEShAEKfDp6CgUSAwoBAhJlEmMKAgoAEj0KOwoCCgASGgoCaWQKBG5hbWUSDgoEKgIQAgoEYgIQARgCOhkKC3Rlc3Rfc2NoZW1hCgp0ZXN0X3RhYmxlGh4aHAgBGgQKAhACIgoaCBIGCgISACIAIgYaBAoCKAMaChIICgQSAggBIgASBE5BTUUyCxBKKgdpc3RobXVz");
    }

    private final PostGreSqlQueryStringBuilder queryBuilder = new PostGreSqlQueryStringBuilder("\"", new PostgreSqlFederationExpressionParser("\""));
    private final Schema schema = SchemaBuilder.newBuilder().addIntField("id").addStringField("name").build();
    private Connection connection;

    @BeforeEach
    void setUp() throws SQLException
    {
        connection = DriverManager.getConnection("jdbc:h2:mem:postgresql_partitions");
        try (Statement statement = connection.createStatement()) {
            statement.execute("DROP SCHEMA IF EXISTS \"test_schema\" CASCADE");
            statement.execute("CREATE SCHEMA \"test_schema\"");
            statement.execute("CREATE TABLE \"test_schema\".\"test_table\" (\"id\" INT PRIMARY KEY, \"name\" VARCHAR(16))");
            for (int id = 1; id <= ROW_COUNT; id++) {
                statement.execute("INSERT INTO \"test_schema\".\"test_table\" VALUES (" + id + ", 'n" + id + "')");
            }
            for (String[] child : CHILDREN) {
                statement.execute("CREATE TABLE \"test_schema\".\"" + child[0] + "\" AS SELECT * FROM \"test_schema\".\"test_table\" WHERE \"id\" BETWEEN "
                        + child[1] + " AND " + child[2]);
            }
        }
    }

    @AfterEach
    void tearDown() throws SQLException
    {
        connection.close();
    }

    @Test
    void testChildPartitionSplitsReturnEveryRowOnceOnQueryPlanPath() throws SQLException
    {
        for (Map.Entry<String, String> plan : PLANS.entrySet()) {
            List<String> expected = run(constraints(plan.getValue()), split("*", "*"));
            List<String> union = new ArrayList<>();
            for (String[] child : CHILDREN) {
                union.addAll(run(constraints(plan.getValue()), split("test_schema", child[0])));
            }
            assertEquals(union.size(), new HashSet<>(union).size(), "duplicate rows for " + plan.getKey() + ": " + union);
            Collections.sort(expected);
            Collections.sort(union);
            assertEquals(expected, union, "rows for " + plan.getKey());
        }
    }

    @Test
    void testChildPartitionSplitsMatchNonPlanPath() throws SQLException
    {
        String plan = PLANS.get("SELECT id, name FROM test_table");
        for (String[] child : CHILDREN) {
            List<String> withPlan = run(constraints(plan), split("test_schema", child[0]));
            List<String> withoutPlan = run(constraints(null), split("test_schema", child[0]));
            assertEquals(Integer.parseInt(child[2]) - Integer.parseInt(child[1]) + 1, withPlan.size(), "rows for " + child[0]);
            assertEquals(withoutPlan, withPlan, "rows for " + child[0]);
        }
    }

    @Test
    void testChildPartitionSplitSqlOnQueryPlanPath() throws SQLException
    {
        Connection mockConnection = mock(Connection.class);
        PreparedStatement charColumns = mock(PreparedStatement.class);
        when(charColumns.executeQuery()).thenReturn(mock(ResultSet.class));
        when(mockConnection.prepareStatement(anyString())).thenReturn(charColumns);
        queryBuilder.buildSql(mockConnection, null, "test_schema", "test_table", schema,
                constraints(PLANS.get("SELECT id, name FROM test_table WHERE id < 5 OR name = 'n17'")), split("test_schema", "test_table_low"));
        ArgumentCaptor<String> sql = ArgumentCaptor.forClass(String.class);
        verify(mockConnection, times(2)).prepareStatement(sql.capture());
        assertEquals("SELECT *\nFROM (SELECT * FROM \"test_schema\".\"test_table_low\") AS \"test_table\"\n"
                + "WHERE \"id\" < ? OR \"name\" = ?", sql.getAllValues().get(1));
    }

    private List<String> run(Constraints constraints, Split split) throws SQLException
    {
        List<String> rows = new ArrayList<>();
        try (PreparedStatement statement = queryBuilder.buildSql(connection, null, "test_schema", "test_table", schema, constraints, split);
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
        Collections.sort(rows);
        return rows;
    }

    private static Constraints constraints(String plan)
    {
        return new Constraints(Collections.emptyMap(), Collections.emptyList(), Collections.emptyList(),
                DEFAULT_NO_LIMIT, Collections.emptyMap(), plan == null ? null : new QueryPlan("", plan));
    }

    private static Split split(String partitionSchema, String partitionName)
    {
        return Split.newBuilder(S3SpillLocation.newBuilder().withBucket("bucket").withPrefix("prefix").build(), null)
                .add(PostGreSqlMetadataHandler.BLOCK_PARTITION_SCHEMA_COLUMN_NAME, partitionSchema)
                .add(PostGreSqlMetadataHandler.BLOCK_PARTITION_COLUMN_NAME, partitionName)
                .build();
    }
}
