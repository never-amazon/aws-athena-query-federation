/*- 
 * #%L 
 * athena-postgresql 
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
package com.amazonaws.athena.connectors.postgresql;

import com.amazonaws.athena.connector.lambda.domain.Split;
import com.amazonaws.athena.connector.lambda.domain.predicate.Constraints;
import com.amazonaws.athena.connectors.jdbc.manager.FederationExpressionParser;
import com.amazonaws.athena.connectors.jdbc.manager.JdbcSplitQueryBuilder;
import com.google.common.base.Strings;
import org.apache.arrow.vector.types.pojo.Schema;
import org.apache.calcite.sql.SqlDialect;
import org.postgresql.jdbc.TimestampUtils;

import java.sql.Connection;
import java.sql.Date;
import java.sql.PreparedStatement;
import java.sql.SQLException;
import java.sql.Timestamp;
import java.sql.Types;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.util.Collections;
import java.util.List;
import java.util.Objects;
import java.util.TimeZone;
import java.util.stream.Collectors;

/**
 * Extends {@link JdbcSplitQueryBuilder} and implements PostGreSql specific SQL clauses for split.
 *
 * PostGreSql partitions through child tables that can be used in a FROM clause.
 */
public class PostGreSqlQueryStringBuilder
        extends JdbcSplitQueryBuilder
{
    private static final String BC_SUFFIX = " BC";
    private static final String UTC_OFFSET = "+00";

    public PostGreSqlQueryStringBuilder(final String quoteCharacters, final FederationExpressionParser federationExpressionParser)
    {
        super(quoteCharacters, federationExpressionParser);
    }

    @Override
    public PreparedStatement buildSql(
            final Connection jdbcConnection,
            final String catalog,
            final String schema,
            final String table,
            final Schema tableSchema,
            final Constraints constraints,
            final Split split)
            throws SQLException
    {
        List<String> charColumns = PostGreSqlMetadataHandler.getCharColumns(jdbcConnection, schema, table);

        String columnNames = tableSchema.getFields().stream()
                .map(field -> {
                    String columnName = field.getName();
                    if (!split.getProperties().containsKey(columnName)) {
                        if (charColumns.contains(columnName)) {
                            return "RTRIM(" + quote(columnName) + ") AS " + quote(columnName);
                        }
                        else {
                            return quote(columnName);
                        }
                    }
                    else {
                        return null;
                    }
                })
                .filter(Objects::nonNull)
                .collect(Collectors.joining(", "));
        return prepareStatementWithSql(jdbcConnection, catalog, schema, table, tableSchema, constraints, split, columnNames);
    }

    @Override
    protected void bindDate(PreparedStatement statement, int index, LocalDate date, Date driverDate) throws SQLException
    {
        statement.setObject(index, date);
    }

    @Override
    protected void bindTimestamp(PreparedStatement statement, int index, LocalDateTime timestamp, Timestamp driverTimestamp) throws SQLException
    {
        statement.setObject(index, utcTimestampText(timestamp), Types.OTHER);
    }

    static String utcTimestampText(LocalDateTime timestamp)
    {
        String text = new TimestampUtils(false, () -> TimeZone.getTimeZone("UTC")).toString(timestamp);
        if (text.endsWith("infinity")) {
            return text;
        }
        if (text.endsWith(BC_SUFFIX)) {
            return text.substring(0, text.length() - BC_SUFFIX.length()) + UTC_OFFSET + BC_SUFFIX;
        }
        return text + UTC_OFFSET;
    }

    @Override
    protected String getFromClauseWithSplit(String catalog, String schema, String table, Split split)
    {
        StringBuilder tableName = new StringBuilder();
        if (!Strings.isNullOrEmpty(catalog)) {
            tableName.append(quote(catalog)).append('.');
        }
        if (!Strings.isNullOrEmpty(schema)) {
            tableName.append(quote(schema)).append('.');
        }
        tableName.append(quote(table));

        String splitTable = getSplitTable(split);
        if (splitTable.isEmpty()) {
            return String.format(" FROM %s ", tableName);
        }

        return String.format(" FROM %s ", splitTable);
    }

    @Override
    protected String getSplitTable(Split split)
    {
        String partitionSchemaName = split.getProperty(PostGreSqlMetadataHandler.BLOCK_PARTITION_SCHEMA_COLUMN_NAME);
        String partitionName = split.getProperty(PostGreSqlMetadataHandler.BLOCK_PARTITION_COLUMN_NAME);

        if (PostGreSqlMetadataHandler.ALL_PARTITIONS.equals(partitionSchemaName) || PostGreSqlMetadataHandler.ALL_PARTITIONS.equals(partitionName)) {
            return emptyString;
        }

        return quote(partitionSchemaName) + "." + quote(partitionName);
    }

    @Override
    protected List<String> getPartitionWhereClauses(final Split split)
    {
        if (split.getProperty(PostGreSqlMetadataHandler.BLOCK_PARTITION_SCHEMA_COLUMN_NAME).equals("*")
                && !split.getProperty(PostGreSqlMetadataHandler.BLOCK_PARTITION_COLUMN_NAME).equals("*")) {
            return Collections.singletonList(split.getProperty(PostGreSqlMetadataHandler.BLOCK_PARTITION_COLUMN_NAME));
        }

        return Collections.emptyList();
    }

    @Override
    protected SqlDialect getSqlDialect()
    {
        return PostgresqlSqlDialect.DEFAULT;
    }

    @Override
    protected SqlDialect getSqlDialect(boolean catalogCasingFilterUpperCase)
    {
        return new PostgresqlSqlDialect(catalogCasingFilterUpperCase);
    }
}
