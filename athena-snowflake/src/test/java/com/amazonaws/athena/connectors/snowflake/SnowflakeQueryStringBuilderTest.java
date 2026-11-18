/*-
 * #%L
 * athena-snowflake
 * %%
 * Copyright (C) 2019 Amazon Web Services
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

import com.amazonaws.athena.connector.lambda.connection.EnvironmentConstants;
import com.amazonaws.athena.connector.lambda.data.BlockAllocator;
import com.amazonaws.athena.connector.lambda.data.BlockAllocatorImpl;
import com.amazonaws.athena.connector.lambda.data.SchemaBuilder;
import com.amazonaws.athena.connector.lambda.domain.Split;
import com.amazonaws.athena.connector.lambda.domain.TableName;
import com.amazonaws.athena.connector.lambda.domain.predicate.Constraints;
import com.amazonaws.athena.connector.lambda.domain.predicate.OrderByField;
import com.amazonaws.athena.connector.lambda.domain.predicate.Range;
import com.amazonaws.athena.connector.lambda.domain.predicate.SortedRangeSet;
import com.amazonaws.athena.connector.lambda.domain.predicate.ValueSet;
import com.amazonaws.athena.connector.lambda.domain.spill.S3SpillLocation;
import com.amazonaws.athena.connectors.jdbc.manager.TypeAndValue;
import org.apache.arrow.vector.types.Types;
import org.apache.arrow.vector.types.pojo.ArrowType;
import org.apache.arrow.vector.types.pojo.Schema;
import org.junit.Before;
import org.junit.Test;

import java.sql.Connection;
import java.sql.PreparedStatement;
import java.sql.SQLException;
import java.util.Arrays;
import java.util.Collections;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;
import static org.mockito.ArgumentMatchers.anyString;
import static org.mockito.Mockito.mock;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

public class SnowflakeQueryStringBuilderTest
{
    private SnowflakeQueryStringBuilder queryBuilder;
    private static final String QUOTE_CHARACTER = "\"";
    private static final BlockAllocator blockAllocator = new BlockAllocatorImpl();
    private static final Constraints NO_CONSTRAINTS = new Constraints(Collections.emptyMap(), Collections.emptyList(), Collections.emptyList(), -1L, Collections.emptyMap(), null);

    @Before
    public void setUp()
    {
        SnowflakeFederationExpressionParser expressionParser = new SnowflakeFederationExpressionParser(QUOTE_CHARACTER);
        queryBuilder = new SnowflakeQueryStringBuilder(QUOTE_CHARACTER, expressionParser);
    }

    @Test
    public void testGetFromClauseWithSplit()
    {
        Split split = Split.newBuilder(
                S3SpillLocation.newBuilder().withBucket("test").withPrefix("test").build(),
                null
        ).build();
        
        String result = queryBuilder.getFromClauseWithSplit("testCatalog", "testSchema", "testTable", split);
        assertTrue(result.contains("\"testSchema\""));
        assertTrue(result.contains("\"testTable\""));
    }

    @Test
    public void testGetFromClauseWithSplitNoSchema()
    {
        Split split = Split.newBuilder(
                S3SpillLocation.newBuilder().withBucket("test").withPrefix("test").build(),
                null
        ).build();
        
        String result = queryBuilder.getFromClauseWithSplit("testCatalog", null, "testTable", split);
        assertTrue(result.contains("\"testTable\""));
        assertTrue(!result.contains("null"));
    }

    @Test
    public void testQuote()
    {
        String result = queryBuilder.quote("testIdentifier");
        assertEquals("\"testIdentifier\"", result);
    }

    @Test
    public void testBuildSqlWithSimpleConstraints() throws SQLException
    {
        Connection mockConnection = mock(Connection.class);
        PreparedStatement mockStatement = mock(PreparedStatement.class);
        when(mockConnection.prepareStatement(anyString())).thenReturn(mockStatement);

        Schema schema = SchemaBuilder.newBuilder()
                .addStringField("col1")
                .addIntField("col2")
                .build();

        Map<String, ValueSet> constraintsMap = new HashMap<>();
        constraintsMap.put("col2", SortedRangeSet.copyOf(Types.MinorType.INT.getType(),
                Arrays.asList(Range.equal(blockAllocator, Types.MinorType.INT.getType(), 42)), false));

        Constraints constraints = new Constraints(constraintsMap, Collections.emptyList(), Collections.emptyList(), -1L, Collections.emptyMap(), null);

        Split split = Split.newBuilder(
                S3SpillLocation.newBuilder().withBucket("test").withPrefix("test").build(),
                null
        ).add("partition", "partition-primary-col1-limit-1000-offset-0").build();

        PreparedStatement result = queryBuilder.buildSql(
                mockConnection,
                "testCatalog",
                "testSchema",
                "testTable",
                schema,
                constraints,
                split
        );

        assertNotNull(result);
        verify(mockConnection).prepareStatement("SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" WHERE (\"col2\" = ?) ORDER BY col1 LIMIT 1000 OFFSET 0");
    }

    @Test
    public void testBuildSqlWithOrderBy() throws SQLException
    {
        Connection mockConnection = mock(Connection.class);
        PreparedStatement mockStatement = mock(PreparedStatement.class);
        when(mockConnection.prepareStatement(anyString())).thenReturn(mockStatement);

        Schema schema = SchemaBuilder.newBuilder()
                .addStringField("col1")
                .addIntField("col2")
                .build();

        List<OrderByField> orderByFields = Arrays.asList(
                new OrderByField("col1", OrderByField.Direction.ASC_NULLS_FIRST),
                new OrderByField("col2", OrderByField.Direction.DESC_NULLS_LAST)
        );

        Constraints constraints = new Constraints(Collections.emptyMap(), Collections.emptyList(), orderByFields, -1L, Collections.emptyMap(), null);

        Split split = Split.newBuilder(
                S3SpillLocation.newBuilder().withBucket("test").withPrefix("test").build(),
                null
        ).add("partition", "partition-primary-col1-limit-1000-offset-0").build();

        PreparedStatement result = queryBuilder.buildSql(
                mockConnection,
                "testCatalog",
                "testSchema",
                "testTable",
                schema,
                constraints,
                split
        );

        assertNotNull(result);
        verify(mockConnection).prepareStatement("SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" ORDER BY \"col1\" ASC NULLS FIRST, \"col2\" DESC NULLS LAST, col1 LIMIT 1000 OFFSET 0");
    }

    @Test
    public void testBuildSqlWithLimit() throws SQLException
    {
        Connection mockConnection = mock(Connection.class);
        PreparedStatement mockStatement = mock(PreparedStatement.class);
        when(mockConnection.prepareStatement(anyString())).thenReturn(mockStatement);

        Schema schema = SchemaBuilder.newBuilder()
                .addStringField("col1")
                .addIntField("col2")
                .build();

        Constraints constraints = new Constraints(Collections.emptyMap(), Collections.emptyList(), Collections.emptyList(), 100L, Collections.emptyMap(), null);

        Split split = Split.newBuilder(
                S3SpillLocation.newBuilder().withBucket("test").withPrefix("test").build(),
                null
        ).add("partition", "test-partition").build();

        PreparedStatement result = queryBuilder.buildSql(
                mockConnection,
                "testCatalog",
                "testSchema",
                "testTable",
                schema,
                constraints,
                split
        );

        assertNotNull(result);
        verify(mockConnection).prepareStatement("SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" LIMIT 100");
    }

    @Test
    public void testBuildSqlWithPartitionConstraints() throws SQLException
    {
        Connection mockConnection = mock(Connection.class);
        PreparedStatement mockStatement = mock(PreparedStatement.class);
        when(mockConnection.prepareStatement(anyString())).thenReturn(mockStatement);

        Schema schema = SchemaBuilder.newBuilder()
                .addStringField("col1")
                .addIntField("col2")
                .build();

        Constraints constraints = new Constraints(Collections.emptyMap(), Collections.emptyList(), Collections.emptyList(), -1L, Collections.emptyMap(), null);

        Split split = Split.newBuilder(
                S3SpillLocation.newBuilder().withBucket("test").withPrefix("test").build(),
                null
        )
        .add("partition", "partition-primary--limit-1000-offset-0")
        .build();

        PreparedStatement result = queryBuilder.buildSql(
                mockConnection,
                "testCatalog",
                "testSchema",
                "testTable",
                schema,
                constraints,
                split
        );

        assertNotNull(result);
        verify(mockConnection).prepareStatement("SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\"");
    }

    @Test
    public void testGetBaseExportSQLString() throws SQLException {
        Schema schema = SchemaBuilder.newBuilder()
                .addStringField("col1")
                .addIntField("col2")
                .addStringField("partition") // Should be excluded
                .build();

        Constraints constraints = new Constraints(Collections.emptyMap(), Collections.emptyList(), Collections.emptyList(), -1L, Collections.emptyMap(), null);

        String result = queryBuilder.getBaseExportSQLString(
                "testCatalog",
                "testSchema",
                "testTable",
                schema,
                constraints
        );

        assertNotNull(result);
        assertTrue(result.contains("SELECT"));
        assertTrue(result.contains("\"col1\""));
        assertTrue(result.contains("\"col2\""));
        assertTrue(result.contains("FROM"));
        assertTrue(result.contains("\"testSchema\".\"testTable\""));
        // Should not contain partition column
        assertTrue(!result.contains("\"partition\""));
        assertEquals("SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\"", result);
    }

    @Test
    public void testGetBaseExportSQLStringWithConstraints() throws SQLException {
        Schema schema = SchemaBuilder.newBuilder()
                .addStringField("col1")
                .addIntField("col2")
                .build();

        Map<String, ValueSet> constraintsMap = new HashMap<>();
        constraintsMap.put("col2", SortedRangeSet.copyOf(Types.MinorType.INT.getType(),
                Arrays.asList(Range.greaterThan(blockAllocator, Types.MinorType.INT.getType(), 10)), false));

        Constraints constraints = new Constraints(constraintsMap, Collections.emptyList(), Collections.emptyList(), 100L, Collections.emptyMap(), null);

        String result = queryBuilder.getBaseExportSQLString(
                "testCatalog",
                "testSchema",
                "testTable",
                schema,
                constraints
        );

        assertNotNull(result);
        assertTrue(result.contains("WHERE"));
        assertTrue(result.contains("LIMIT"));
        assertEquals("SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" WHERE ((\"col2\" > 10)) LIMIT 100", result);
    }

    @Test
    public void testGetBaseExportSQLStringWithOrderBy() throws SQLException {
        Schema schema = SchemaBuilder.newBuilder()
                .addStringField("col1")
                .addIntField("col2")
                .build();

        List<OrderByField> orderByFields = Arrays.asList(
                new OrderByField("col1", OrderByField.Direction.ASC_NULLS_FIRST)
        );

        Constraints constraints = new Constraints(Collections.emptyMap(), Collections.emptyList(), orderByFields, -1L, Collections.emptyMap(), null);

        String result = queryBuilder.getBaseExportSQLString(
                "testCatalog",
                "testSchema",
                "testTable",
                schema,
                constraints
        );

        assertNotNull(result);
        assertTrue(result.contains("ORDER BY"));
        assertTrue(result.contains("\"col1\""));
        assertEquals("SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" ORDER BY \"col1\" ASC NULLS FIRST", result);
    }

    @Test
    public void testGetBaseExportSQLStringNoCatalog() throws SQLException {
        Schema schema = SchemaBuilder.newBuilder()
                .addStringField("col1")
                .addIntField("col2")
                .build();

        Constraints constraints = new Constraints(Collections.emptyMap(), Collections.emptyList(), Collections.emptyList(), -1L, Collections.emptyMap(), null);

        String result = queryBuilder.getBaseExportSQLString(
                null,
                "testSchema",
                "testTable",
                schema,
                constraints
        );

        assertNotNull(result);
        assertTrue(result.contains("\"testSchema\".\"testTable\""));
        assertTrue(!result.contains("null"));
        assertEquals("SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\"", result);
    }

    @Test
    public void testBuildSqlWithComplexPartition() throws SQLException
    {
        Connection mockConnection = mock(Connection.class);
        PreparedStatement mockStatement = mock(PreparedStatement.class);
        when(mockConnection.prepareStatement(anyString())).thenReturn(mockStatement);

        Schema schema = SchemaBuilder.newBuilder()
                .addStringField("col1")
                .addIntField("col2")
                .build();

        Constraints constraints = new Constraints(Collections.emptyMap(), Collections.emptyList(), Collections.emptyList(), -1L, Collections.emptyMap(), null);

        Split split = Split.newBuilder(
                S3SpillLocation.newBuilder().withBucket("test").withPrefix("test").build(),
                null
        )
        .add("partition", "partition-primary-\"id\",\"name\"-limit-5000-offset-10000")
        .build();

        PreparedStatement result = queryBuilder.buildSql(
                mockConnection,
                "testCatalog",
                "testSchema",
                "testTable",
                schema,
                constraints,
                split
        );

        assertNotNull(result);
        verify(mockConnection).prepareStatement("SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" ORDER BY \"id\",\"name\" LIMIT 5000 OFFSET 10000");
    }

    @Test
    public void testBuildSqlWithAllPartition() throws SQLException
    {
        Connection mockConnection = mock(Connection.class);
        PreparedStatement mockStatement = mock(PreparedStatement.class);
        when(mockConnection.prepareStatement(anyString())).thenReturn(mockStatement);

        Schema schema = SchemaBuilder.newBuilder()
                .addStringField("col1")
                .addIntField("col2")
                .build();

        Constraints constraints = new Constraints(Collections.emptyMap(), Collections.emptyList(), Collections.emptyList(), -1L, Collections.emptyMap(), null);

        Split split = Split.newBuilder(
                S3SpillLocation.newBuilder().withBucket("test").withPrefix("test").build(),
                null
        )
        .add("partition", "*") // All partitions
        .build();

        PreparedStatement result = queryBuilder.buildSql(
                mockConnection,
                "testCatalog",
                "testSchema",
                "testTable",
                schema,
                constraints,
                split
        );

        assertNotNull(result);
        verify(mockConnection).prepareStatement("SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\"");
    }

    @Test
    public void testAppendLimitOffsetWithNullSplit()
    {
        String result = queryBuilder.appendOrderByLimitOffset(null, NO_CONSTRAINTS);
        assertEquals("", result);
    }

    @Test
    public void testAppendLimitOffsetWithEmptySplit()
    {
        Split split = Split.newBuilder(
                S3SpillLocation.newBuilder().withBucket("test").withPrefix("test").build(),
                null
        ).build();

        String result = queryBuilder.appendOrderByLimitOffset(split, NO_CONSTRAINTS);
        assertEquals("", result);
    }

    @Test
    public void testAppendLimitOffsetWithMissingMarkers()
    {
        Split split = Split.newBuilder(
                S3SpillLocation.newBuilder().withBucket("test").withPrefix("test").build(),
                null
        ).add("partition", "some-random-value").build();

        String result = queryBuilder.appendOrderByLimitOffset(split, NO_CONSTRAINTS);
        assertEquals("", result);
    }

    @Test
    public void testAppendLimitOffsetWithMalformedPartition()
    {
        // Markers in wrong order: offset before limit
        Split split = Split.newBuilder(
                S3SpillLocation.newBuilder().withBucket("test").withPrefix("test").build(),
                null
        ).add("partition", "partition-offset-0-limit-100-primary-col1").build();

        String result = queryBuilder.appendOrderByLimitOffset(split, NO_CONSTRAINTS);
        assertEquals("", result);
    }

    @Test
    public void testAppendLimitOffsetWithEmptyPrimaryKey()
    {
        Split split = Split.newBuilder(
                S3SpillLocation.newBuilder().withBucket("test").withPrefix("test").build(),
                null
        ).add("partition", "partition-primary--limit-1000-offset-0").build();

        String result = queryBuilder.appendOrderByLimitOffset(split, NO_CONSTRAINTS);
        assertEquals("", result);
    }

    @Test
    public void testAppendLimitOffsetWithNonNumericLimit()
    {
        Split split = Split.newBuilder(
                S3SpillLocation.newBuilder().withBucket("test").withPrefix("test").build(),
                null
        ).add("partition", "partition-primary-col1-limit-abc-offset-0").build();

        String result = queryBuilder.appendOrderByLimitOffset(split, NO_CONSTRAINTS);
        assertEquals("", result);
    }

    @Test
    public void testAppendLimitOffsetWithNonNumericOffset()
    {
        Split split = Split.newBuilder(
                S3SpillLocation.newBuilder().withBucket("test").withPrefix("test").build(),
                null
        ).add("partition", "partition-primary-col1-limit-1000-offset-xyz").build();

        String result = queryBuilder.appendOrderByLimitOffset(split, NO_CONSTRAINTS);
        assertEquals("", result);
    }

    @Test
    public void testAppendLimitOffsetWithValidPartition()
    {
        Split split = Split.newBuilder(
                S3SpillLocation.newBuilder().withBucket("test").withPrefix("test").build(),
                null
        ).add("partition", "partition-primary-\"id\"-limit-1000-offset-500").build();

        String result = queryBuilder.appendOrderByLimitOffset(split, NO_CONSTRAINTS);
        assertEquals(" ORDER BY \"id\" LIMIT 1000 OFFSET 500", result);
    }

    @Test
    public void testAppendLimitOffsetWithCompositePrimaryKey()
    {
        Split split = Split.newBuilder(
                S3SpillLocation.newBuilder().withBucket("test").withPrefix("test").build(),
                null
        ).add("partition", "partition-primary-\"col1\",\"col2\"-limit-5000-offset-10000").build();

        String result = queryBuilder.appendOrderByLimitOffset(split, NO_CONSTRAINTS);
        assertEquals(" ORDER BY \"col1\",\"col2\" LIMIT 5000 OFFSET 10000", result);
    }

    @Test
    public void testAppendLimitOffsetWithCasingFilterBeforePartition()
    {
        // Direct-query splits can carry the casing filter too, and SplitSerDe keeps the key order it receives.
        Map<String, String> properties = new LinkedHashMap<>();
        properties.put(EnvironmentConstants.CATALOG_CASING_FILTER, EnvironmentConstants.UPPERCASE_ONLY);
        properties.put("partition", "partition-primary-\"id\"-limit-100-offset-200");
        Split split = new Split(S3SpillLocation.newBuilder().withBucket("test").withPrefix("test").build(), null, properties);

        String result = queryBuilder.appendOrderByLimitOffset(split, NO_CONSTRAINTS);
        assertTrue(result.contains("ORDER BY \"id\""));
        assertTrue(result.contains("LIMIT 100 OFFSET 200"));
        assertEquals(" ORDER BY \"id\" LIMIT 100 OFFSET 200", result);
    }

    @Test
    public void testBuildSqlWithOrderByLimitAndPagination() throws SQLException
    {
        Connection mockConnection = mock(Connection.class);
        when(mockConnection.prepareStatement(anyString())).thenReturn(mock(PreparedStatement.class));
        Map<String, ValueSet> constraintsMap = new HashMap<>();
        constraintsMap.put("col2", SortedRangeSet.copyOf(Types.MinorType.INT.getType(),
                Arrays.asList(Range.greaterThan(blockAllocator, Types.MinorType.INT.getType(), 10)), false));
        List<OrderByField> orderByFields = Arrays.asList(new OrderByField("col2", OrderByField.Direction.DESC_NULLS_LAST));
        Constraints constraints = new Constraints(constraintsMap, Collections.emptyList(), orderByFields, 5000L, Collections.emptyMap(), null);

        queryBuilder.buildSql(mockConnection, "testCatalog", "testSchema", "testTable", twoColumnSchema(), constraints,
                paginatedSplit("partition-primary-\"id\"-limit-1000-offset-3000"));

        verify(mockConnection).prepareStatement("SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" WHERE ((\"col2\" > ?)) ORDER BY \"col2\" DESC NULLS LAST, \"id\" LIMIT 1000 OFFSET 3000");
    }

    @Test
    public void testBuildSqlWithLimitBelowPageSizeAndPagination() throws SQLException
    {
        Connection mockConnection = mock(Connection.class);
        when(mockConnection.prepareStatement(anyString())).thenReturn(mock(PreparedStatement.class));
        Constraints constraints = new Constraints(Collections.emptyMap(), Collections.emptyList(), Collections.emptyList(), 100L, Collections.emptyMap(), null);

        queryBuilder.buildSql(mockConnection, "testCatalog", "testSchema", "testTable", twoColumnSchema(), constraints,
                paginatedSplit("partition-primary-\"id\"-limit-1000-offset-2000"));

        verify(mockConnection).prepareStatement("SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" ORDER BY \"id\" LIMIT 100 OFFSET 2000");
    }

    @Test
    public void testGetBaseExportSQLStringWithOrderByAndLimit() throws SQLException
    {
        List<OrderByField> orderByFields = Arrays.asList(new OrderByField("col1", OrderByField.Direction.ASC_NULLS_FIRST));
        Constraints constraints = new Constraints(Collections.emptyMap(), Collections.emptyList(), orderByFields, 10L, Collections.emptyMap(), null);

        String result = queryBuilder.getBaseExportSQLString("testCatalog", "testSchema", "testTable", twoColumnSchema(), constraints);

        assertEquals("SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" ORDER BY \"col1\" ASC NULLS FIRST LIMIT 10", result);
    }

    private static Schema twoColumnSchema()
    {
        return SchemaBuilder.newBuilder()
                .addStringField("col1")
                .addIntField("col2")
                .build();
    }

    private static Split paginatedSplit(String partition)
    {
        return Split.newBuilder(S3SpillLocation.newBuilder().withBucket("test").withPrefix("test").build(), null)
                .add("partition", partition)
                .build();
    }

    @Test
    public void testGetSqlDialect()
    {
        org.apache.calcite.sql.SqlDialect dialect = queryBuilder.getSqlDialect();
        assertNotNull(dialect);
        assertTrue(dialect instanceof org.apache.calcite.sql.dialect.SnowflakeSqlDialect);
    }

    @Test
    public void testGetSqlDialectWithCasingFilter()
    {
        org.apache.calcite.sql.SqlDialect dialect = queryBuilder.getSqlDialect(true);
        assertNotNull(dialect);
        assertTrue(dialect instanceof SnowflakeDialect);
    }

    @Test
    public void testGetSqlDialectWithCasingFilterFalse()
    {
        org.apache.calcite.sql.SqlDialect dialect = queryBuilder.getSqlDialect(false);
        assertNotNull(dialect);
        assertTrue(dialect instanceof SnowflakeDialect);
    }

    private static TypeAndValue varchar(String value)
    {
        return new TypeAndValue(new ArrowType.Utf8(), value);
    }

    @Test
    public void testExpandSqlKeepsEraAndSignInDateAndTimestampLiterals()
    {
        String sql = "SELECT * FROM \"t\" WHERE \"d\" = ? AND \"e\" = ? AND \"ts\" = ?";
        List<TypeAndValue> values = Arrays.asList(
                new TypeAndValue(org.apache.arrow.vector.types.Types.MinorType.DATEDAY.getType(), -735_160L),
                new TypeAndValue(org.apache.arrow.vector.types.Types.MinorType.DATEDAY.getType(), 2_932_897L),
                new TypeAndValue(org.apache.arrow.vector.types.Types.MinorType.DATEMILLI.getType(), -63_517_780_800_000L));
        assertEquals("SELECT * FROM \"t\" WHERE \"d\" = DATE '-0043-03-15' AND \"e\" = DATE '+10000-01-01' AND \"ts\" = TIMESTAMP '-0043-03-15T12:00:00Z'",
                queryBuilder.expandSql(sql, values));
    }

    @Test
    public void testExpandSqlEscapesBackslashAndQuote()
    {
        String sql = "SELECT * FROM \"t\" WHERE \"a\" = ? AND \"b\" = ? AND \"c\" = ?";
        List<TypeAndValue> values = Arrays.asList(varchar("x\\"), varchar("\\' OR 1=1 --"), varchar("it's"));
        assertEquals("SELECT * FROM \"t\" WHERE \"a\" = 'x\\\\' AND \"b\" = '\\\\'' OR 1=1 --' AND \"c\" = 'it''s'",
                queryBuilder.expandSql(sql, values));
    }

    @Test
    public void testExpandSqlKeepsDollarAndBackslashInValuesLiteral()
    {
        String sql = "SELECT * FROM \"t\" WHERE \"a\" = ? AND \"b\" = ?";
        List<TypeAndValue> values = Arrays.asList(varchar("$1"), varchar("a\\$0b"));
        assertEquals("SELECT * FROM \"t\" WHERE \"a\" = '$1' AND \"b\" = 'a\\\\$0b'",
                queryBuilder.expandSql(sql, values));
    }

    @Test
    public void testExpandSqlKeepsQuestionMarksInsideLiteralsAndIdentifiers()
    {
        String sql = "SELECT \"q?\" FROM \"t\" WHERE \"a\" = ? AND \"b\" = '?' AND \"c\" = ?";
        List<TypeAndValue> values = Arrays.asList(varchar("a?b"), varchar("c"));
        assertEquals("SELECT \"q?\" FROM \"t\" WHERE \"a\" = 'a?b' AND \"b\" = '?' AND \"c\" = 'c'",
                queryBuilder.expandSql(sql, values));
    }

    @Test
    public void bindTimestampAndDateSendExactEpochValuesThroughTheDriver() throws Exception
    {
        Map<String, net.snowflake.client.internal.core.ParameterBindingDTO> bindings = new HashMap<>();
        net.snowflake.client.internal.api.implementation.statement.SnowflakePreparedStatementImpl statement = driverStatement(bindings);
        java.time.LocalDateTime start = java.time.LocalDateTime.of(-43, 3, 15, 12, 0);
        java.time.LocalDateTime end = java.time.LocalDateTime.of(1583, 1, 1, 0, 0);
        int checked = 0;
        for (java.time.LocalDateTime value = start; value.isBefore(end); value = value.plusDays(5).plusHours(1).plusNanos(1_500_000)) {
            assertBinds(statement, bindings, value);
            checked++;
        }
        for (java.time.LocalDateTime value = java.time.LocalDateTime.of(1582, 9, 25, 0, 0); value.isBefore(java.time.LocalDateTime.of(1582, 10, 5, 0, 0)); value = value.plusHours(7)) {
            assertBinds(statement, bindings, value);
        }
        for (java.time.LocalDateTime value : new java.time.LocalDateTime[] {java.time.LocalDateTime.of(1000, 1, 1, 12, 0),
                java.time.LocalDateTime.of(1582, 9, 24, 23, 59, 59, 999_000_000), java.time.LocalDateTime.of(1582, 10, 15, 0, 0),
                java.time.LocalDateTime.of(1969, 12, 31, 23, 59, 59, 500_000_000), java.time.LocalDateTime.of(2020, 6, 1, 12, 0, 0, 123_456_789)}) {
            assertBinds(statement, bindings, value);
        }
        assertTrue(checked > 40_000);

        java.time.LocalDateTime shifted = java.time.LocalDateTime.of(1000, 1, 1, 12, 0);
        statement.setTimestamp(1, new java.sql.Timestamp(shifted.toInstant(java.time.ZoneOffset.UTC).toEpochMilli()));
        assertEquals(java.math.BigDecimal.valueOf(shifted.minusDays(5).toEpochSecond(java.time.ZoneOffset.UTC)).scaleByPowerOfTen(9).toPlainString(),
                bindings.get("1").getValue());
    }

    @Test
    public void bindTimestampAndDateSendExactEpochValuesBefore3500Bc() throws Exception
    {
        Map<String, net.snowflake.client.internal.core.ParameterBindingDTO> bindings = new HashMap<>();
        net.snowflake.client.internal.api.implementation.statement.SnowflakePreparedStatementImpl statement = driverStatement(bindings);
        queryBuilder.bindTimestamp(statement, 1, java.time.LocalDateTime.of(-5000, 1, 1, 0, 30), null);
        assertEquals("-219951934200000000000", bindings.get("1").getValue());
        queryBuilder.bindDate(statement, 2, java.time.LocalDate.of(-5000, 1, 1), null);
        assertEquals("-219951936000000", bindings.get("2").getValue());
        for (java.time.LocalDateTime value = java.time.LocalDateTime.of(-20000, 1, 1, 0, 30); value.getYear() < 1; value = value.plusDays(97).plusHours(5).plusNanos(1_500_000)) {
            assertBinds(statement, bindings, value);
        }
        for (java.time.LocalDateTime value = java.time.LocalDateTime.of(-1000, 12, 1, 0, 30); value.isBefore(java.time.LocalDateTime.of(-999, 1, 10, 0, 0)); value = value.plusHours(5)) {
            assertBinds(statement, bindings, value);
        }
    }

    @Test
    public void bindTimestampUsesTheSessionTimestampTypeMapping() throws Exception
    {
        java.time.LocalDateTime[] timestamps = {java.time.LocalDateTime.of(2020, 6, 1, 12, 0, 0, 123_456_789),
                java.time.LocalDateTime.of(1969, 12, 31, 23, 59, 59, 500_000_000), java.time.LocalDateTime.of(1000, 1, 1, 12, 0)};
        String[] nanos = {"1591012800123456789", "-500000000", "-30610180800000000000"};
        Object[][] cases = {
            {net.snowflake.client.api.resultset.SnowflakeType.TIMESTAMP_NTZ, ""},
            {net.snowflake.client.api.resultset.SnowflakeType.TIMESTAMP_LTZ, ""},
            {net.snowflake.client.api.resultset.SnowflakeType.TIMESTAMP_TZ, " 1440"},
        };
        for (Object[] mapping : cases) {
            Map<String, net.snowflake.client.internal.core.ParameterBindingDTO> bindings = new HashMap<>();
            net.snowflake.client.internal.api.implementation.statement.SnowflakePreparedStatementImpl statement =
                    driverStatement(bindings, (net.snowflake.client.api.resultset.SnowflakeType) mapping[0]);
            for (int i = 0; i < timestamps.length; i++) {
                queryBuilder.bindTimestamp(statement, 1, timestamps[i], null);
                assertEquals(mapping[0].toString(), bindings.get("1").getType());
                assertEquals(nanos[i] + mapping[1], bindings.get("1").getValue());
            }
        }
    }

    private void assertBinds(net.snowflake.client.internal.api.implementation.statement.SnowflakePreparedStatementImpl statement,
            Map<String, net.snowflake.client.internal.core.ParameterBindingDTO> bindings, java.time.LocalDateTime value) throws SQLException
    {
        queryBuilder.bindTimestamp(statement, 1, value, null);
        java.math.BigDecimal nanos = java.math.BigDecimal.valueOf(value.toEpochSecond(java.time.ZoneOffset.UTC)).scaleByPowerOfTen(9)
                .add(java.math.BigDecimal.valueOf(value.getNano()));
        assertEquals(value.toString(), String.valueOf(nanos), bindings.get("1").getValue());
        assertEquals("TIMESTAMP_LTZ", bindings.get("1").getType());
        queryBuilder.bindDate(statement, 2, value.toLocalDate(), null);
        assertEquals(value.toLocalDate().toString(), String.valueOf(value.toLocalDate().toEpochDay() * 86_400_000L), bindings.get("2").getValue());
        assertEquals("DATE", bindings.get("2").getType());
    }

    private static net.snowflake.client.internal.api.implementation.statement.SnowflakePreparedStatementImpl driverStatement(
            Map<String, net.snowflake.client.internal.core.ParameterBindingDTO> bindings) throws Exception
    {
        return driverStatement(bindings, net.snowflake.client.api.resultset.SnowflakeType.TIMESTAMP_LTZ);
    }

    private static net.snowflake.client.internal.api.implementation.statement.SnowflakePreparedStatementImpl driverStatement(
            Map<String, net.snowflake.client.internal.core.ParameterBindingDTO> bindings,
            net.snowflake.client.api.resultset.SnowflakeType timestampMapping) throws Exception
    {
        net.snowflake.client.internal.api.implementation.statement.SnowflakePreparedStatementImpl statement = mock(
                net.snowflake.client.internal.api.implementation.statement.SnowflakePreparedStatementImpl.class, org.mockito.Mockito.CALLS_REAL_METHODS);
        java.lang.reflect.Field parameterBindings = net.snowflake.client.internal.api.implementation.statement.SnowflakePreparedStatementImpl.class
                .getDeclaredField("parameterBindings");
        parameterBindings.setAccessible(true);
        parameterBindings.set(statement, bindings);
        net.snowflake.client.internal.api.implementation.connection.SnowflakeConnectionImpl connection = mock(
                net.snowflake.client.internal.api.implementation.connection.SnowflakeConnectionImpl.class, org.mockito.Mockito.RETURNS_DEEP_STUBS);
        when(connection.getSFBaseSession(org.mockito.ArgumentMatchers.any()).getTimestampMappedType())
                .thenReturn(timestampMapping);
        java.lang.reflect.Field connectionField = net.snowflake.client.internal.api.implementation.statement.SnowflakeStatementImpl.class
                .getDeclaredField("connection");
        connectionField.setAccessible(true);
        connectionField.set(statement, connection);
        java.lang.reflect.Field closedField = net.snowflake.client.internal.api.implementation.statement.SnowflakeStatementImpl.class
                .getDeclaredField("isClosed");
        closedField.setAccessible(true);
        closedField.set(statement, false);
        return statement;
    }
}
