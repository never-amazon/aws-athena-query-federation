/*-
 * #%L
 * athena-snowflake
 * %%
 * Copyright (C) 2019 - 2022 Amazon Web Services
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
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.mockito.ArgumentCaptor;

import java.sql.Connection;
import java.sql.PreparedStatement;
import java.sql.SQLException;
import java.util.Collections;

import static com.amazonaws.athena.connector.lambda.domain.predicate.Constraints.DEFAULT_NO_LIMIT;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.mockito.ArgumentMatchers.anyString;
import static org.mockito.Mockito.mock;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

/**
 * Builds split SQL from real Substrait plans, which SnowflakeQueryStringBuilder renders through
 * JdbcSplitQueryBuilder.prepareStatementWithCalciteSql.
 */
class SnowflakeSubstraitQueryBuilderTest
{
    // SELECT id, name FROM test_schema.test_table WHERE id > 10
    private static final String PLAN_WHERE_ID_GREATER_THAN_10 =
            "Ch4IARIaL2Z1bmN0aW9uc19jb21wYXJpc29uLnlhbWwSEhoQCAEQARoKZ3Q6YW55X2FueRqYARKVAQqIATqFAQoGEgQKAgIDEmUSYwoCCgASPQo7CgIKABIaCgJpZAoEbmFtZRIOCgQqAhACCgRiAhACGAI6GQoLdGVzdF9zY2hlbWEKCnRlc3RfdGFibGUaHhocCAEaBAoCEAIiChoIEgYKAhIAIgAiBhoECgIoChoIEgYKAhIAIgAaChIICgQSAggBIgASAklEEgROQU1FMgsQSioHaXN0aG11cw==";
    // SELECT id, name FROM test_schema.test_table WHERE name = 'abc'
    private static final String PLAN_WHERE_NAME_EQUALS_ABC =
            "Ch4IARIaL2Z1bmN0aW9uc19jb21wYXJpc29uLnlhbWwSFRoTCAEQARoNZXF1YWw6YW55X2FueRqdARKaAQqNATqKAQoGEgQKAgIDEmoSaAoCCgASPQo7CgIKABIaCgJpZAoEbmFtZRIOCgQqAhACCgRiAhACGAI6GQoLdGVzdF9zY2hlbWEKCnRlc3RfdGFibGUaIxohCAEaBAoCEAIiDBoKEggKBBICCAEiACIJGgcKBWIDYWJjGggSBgoCEgAiABoKEggKBBICCAEiABICSUQSBE5BTUUyCxBKKgdpc3RobXVz";
    // SELECT id, name FROM test_schema.test_table LIMIT 10
    private static final String PLAN_LIMIT_10 =
            "GnkSdwprGmkKAgoAEl86XQoGEgQKAgIDEj0KOwoCCgASGgoCaWQKBG5hbWUSDgoEKgIQAgoEYgIQAhgCOhkKC3Rlc3Rfc2NoZW1hCgp0ZXN0X3RhYmxlGggSBgoCEgAiABoKEggKBBICCAEiABgAIAoSAklEEgROQU1FMgsQSioHaXN0aG11cw==";

    private final SnowflakeQueryStringBuilder queryBuilder = new SnowflakeQueryStringBuilder("\"", null);
    private final Schema schema = SchemaBuilder.newBuilder().addIntField("id").addStringField("name").build();
    private Connection connection;
    private PreparedStatement statement;

    @BeforeEach
    void setUp() throws SQLException
    {
        connection = mock(Connection.class);
        statement = mock(PreparedStatement.class);
        when(connection.prepareStatement(anyString())).thenReturn(statement);
    }

    @Test
    void testBuildSplitSql_WithQueryPlan_Success() throws SQLException
    {
        assertEquals("SELECT *\nFROM \"test_schema\".\"test_table\"\nWHERE \"id\" > ?", buildSql(PLAN_WHERE_ID_GREATER_THAN_10));
        verify(statement).setInt(1, 10);
    }

    @Test
    void testBuildSplitSql_WithParameterBinding() throws SQLException
    {
        assertEquals("SELECT *\nFROM \"test_schema\".\"test_table\"\nWHERE \"name\" = ?", buildSql(PLAN_WHERE_NAME_EQUALS_ABC));
        verify(statement).setString(1, "abc");
    }

    @Test
    void testBuildSplitSql_WithQueryPlan_NoWhereClause() throws SQLException
    {
        // AnsiSqlDialect renders FETCH NEXT 10 ROWS ONLY, so LIMIT 10 shows that the Snowflake dialect was used.
        assertEquals("SELECT *\nFROM \"test_schema\".\"test_table\"\nLIMIT 10", buildSql(PLAN_LIMIT_10));
    }

    private String buildSql(String substraitPlan) throws SQLException
    {
        Constraints constraints = new Constraints(Collections.emptyMap(), Collections.emptyList(), Collections.emptyList(),
                DEFAULT_NO_LIMIT, Collections.emptyMap(), new QueryPlan("", substraitPlan));
        Split split = Split.newBuilder(S3SpillLocation.newBuilder().withBucket("bucket").withPrefix("prefix").build(), null).build();

        queryBuilder.buildSql(connection, "test_catalog", "test_schema", "test_table", schema, constraints, split);

        ArgumentCaptor<String> sql = ArgumentCaptor.forClass(String.class);
        verify(connection).prepareStatement(sql.capture());
        return sql.getValue();
    }
}
