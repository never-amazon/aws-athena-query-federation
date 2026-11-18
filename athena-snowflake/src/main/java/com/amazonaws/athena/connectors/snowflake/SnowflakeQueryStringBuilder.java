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

import com.amazonaws.athena.connector.lambda.domain.Split;
import com.amazonaws.athena.connector.lambda.domain.predicate.Constraints;
import com.amazonaws.athena.connectors.jdbc.manager.FederationExpressionParser;
import com.amazonaws.athena.connectors.jdbc.manager.JdbcSplitQueryBuilder;
import com.amazonaws.athena.connectors.jdbc.manager.TypeAndValue;
import com.google.common.annotations.VisibleForTesting;
import com.google.common.base.Strings;
import org.apache.arrow.vector.types.Types;
import org.apache.arrow.vector.types.pojo.Field;
import org.apache.arrow.vector.types.pojo.Schema;
import org.apache.calcite.sql.SqlDialect;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.sql.Date;
import java.sql.PreparedStatement;
import java.sql.SQLException;
import java.sql.Timestamp;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.time.ZoneOffset;
import java.util.ArrayList;
import java.util.Calendar;
import java.util.Collections;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.SimpleTimeZone;
import java.util.stream.Collectors;

import static com.amazonaws.athena.connectors.snowflake.SnowflakeConstants.BLOCK_PARTITION_COLUMN_NAME;
import static com.amazonaws.athena.connectors.snowflake.SnowflakeConstants.DOUBLE_QUOTE_CHAR;
import static com.amazonaws.athena.connectors.snowflake.SnowflakeConstants.PARTITION_LIMIT_MARKER;
import static com.amazonaws.athena.connectors.snowflake.SnowflakeConstants.PARTITION_OFFSET_MARKER;
import static com.amazonaws.athena.connectors.snowflake.SnowflakeConstants.PARTITION_PRIMARY_MARKER;
import static com.amazonaws.athena.connectors.snowflake.SnowflakeConstants.SINGLE_QUOTE_CHAR;
import static java.lang.Math.toIntExact;

/**
 * Extends {@link JdbcSplitQueryBuilder} and implements MySql specific SQL clauses for split.
 *
 * MySql provides named partitions which can be used in a FROM clause.
 */
public class SnowflakeQueryStringBuilder
        extends JdbcSplitQueryBuilder
{
    private static final Logger LOGGER = LoggerFactory.getLogger(SnowflakeQueryStringBuilder.class);
    private static final long MILLIS_PER_DAY = 86_400_000L;

    public SnowflakeQueryStringBuilder(final String quoteCharacters, final FederationExpressionParser federationExpressionParser)
    {
        super(quoteCharacters, federationExpressionParser);
    }

    @Override
    protected void bindDate(PreparedStatement statement, int index, LocalDate date, Date driverDate) throws SQLException
    {
        long epochMillis = date.toEpochDay() * MILLIS_PER_DAY;
        statement.setDate(index, new Date(epochMillis), fixedOffsetCalendar(SnowflakeRecordHandler.julianAdjustment(epochMillis)));
    }

    @Override
    protected void bindTimestamp(PreparedStatement statement, int index, LocalDateTime timestamp, Timestamp driverTimestamp) throws SQLException
    {
        statement.setTimestamp(index, driverTimestamp(timestamp), fixedOffsetCalendar(0));
    }

    @VisibleForTesting
    static Timestamp driverTimestamp(LocalDateTime value)
    {
        long seconds = value.toEpochSecond(ZoneOffset.UTC);
        long millisOfSecond = value.getNano() / 1_000_000;
        Timestamp timestamp = new Timestamp((seconds < 0 && millisOfSecond > 0 ? seconds - 1 : seconds) * 1000 + millisOfSecond);
        timestamp.setNanos(value.getNano());
        return timestamp;
    }

    private static Calendar fixedOffsetCalendar(long offsetMillis)
    {
        return Calendar.getInstance(new SimpleTimeZone(toIntExact(offsetMillis), "UTC"));
    }

    @Override
    protected String getFromClauseWithSplit(String catalog, String schema, String table, Split split)
    {
        StringBuilder tableName = new StringBuilder();
        if (!Strings.isNullOrEmpty(schema)) {
            tableName.append(quote(schema)).append('.');
        }
        tableName.append(quote(table));
        return " FROM " + tableName;
    }

    @Override
    protected List<String> getPartitionWhereClauses(Split split)
    {
        return Collections.emptyList();
    }

    public String getBaseExportSQLString(
            final String catalog,
            final String schema,
            final String table,
            final Schema tableSchema,
            final Constraints constraints)
            throws SQLException
    {
        String columnNames = tableSchema.getFields().stream()
                .map(Field::getName)
                .filter(name -> !name.equalsIgnoreCase(BLOCK_PARTITION_COLUMN_NAME))
                .map(this::quote)
                .collect(Collectors.joining(", "));

        // We compute the base Export SQL String at GetSplit stage, at the time we don't have column projection hence getting everything.
        if (columnNames.isEmpty()) {
            columnNames = "*";
        }

        List<TypeAndValue> accumulator = new ArrayList<>();
        String sqlBaseString = this.buildSQLStringLiteral(catalog, schema, table, tableSchema, constraints, new Split(null, null, Map.of()), columnNames, accumulator);

        sqlBaseString = expandSql(sqlBaseString, accumulator);
        LOGGER.info("Expanded Generated SQL : {}", sqlBaseString);
        return sqlBaseString;
    }

    @VisibleForTesting
    String expandSql(String sql, List<TypeAndValue> accumulator)
    {
        if (Strings.isNullOrEmpty(sql)) {
            return null;
        }
        
        List<String> sqlLiterals = new ArrayList<>();
        for (TypeAndValue typeAndValue : accumulator) {
            String sqlLiteral;
            
            if (typeAndValue.getValue() == null) {
                sqlLiteral = "NULL";
            }
            else {
                Types.MinorType minorType = Types.getMinorTypeForArrowType(typeAndValue.getType());
                
                switch (minorType) {
                    case DATEDAY:
                        long days = ((Number) typeAndValue.getValue()).longValue();
                        sqlLiteral = "DATE " + singleQuote(LocalDate.ofEpochDay(days).toString());
                        break;
                    case DATEMILLI:
                        long millis = ((Number) typeAndValue.getValue()).longValue();
                        sqlLiteral = "TIMESTAMP " + singleQuote(java.time.Instant.ofEpochMilli(millis).toString());
                        break;
                    case TIMESTAMPMILLITZ:
                    case TIMESTAMPMICROTZ:
                        if (typeAndValue.getValue() instanceof java.sql.Timestamp) {
                            sqlLiteral = "TIMESTAMP " + singleQuote(typeAndValue.getValue().toString());
                        }
                        else {
                            long tsMillis = ((Number) typeAndValue.getValue()).longValue();
                            sqlLiteral = "TIMESTAMP " + singleQuote(java.time.Instant.ofEpochMilli(tsMillis).toString());
                        }
                        break;
                    case INT:
                    case SMALLINT:
                    case TINYINT:
                    case BIGINT:
                    case FLOAT4:
                    case FLOAT8:
                    case DECIMAL:
                        sqlLiteral = typeAndValue.getValue().toString();
                        break;
                    default:
                        sqlLiteral = singleQuote(typeAndValue.getValue().toString());
                        break;
                }
            }
            
            sqlLiterals.add(sqlLiteral);
        }
        return replacePlaceholders(sql, sqlLiterals);
    }

    private static String replacePlaceholders(String sql, List<String> sqlLiterals)
    {
        StringBuilder expanded = new StringBuilder(sql.length());
        int next = 0;
        char openQuote = 0;
        for (int i = 0; i < sql.length(); i++) {
            char c = sql.charAt(i);
            if (openQuote != 0) {
                if (c == '\\' && openQuote == '\'' && i + 1 < sql.length()) {
                    expanded.append(c).append(sql.charAt(++i));
                    continue;
                }
                if (c == openQuote) {
                    openQuote = 0;
                }
                expanded.append(c);
            }
            else if (c == '\'' || c == '"') {
                openQuote = c;
                expanded.append(c);
            }
            else if (c == '?' && next < sqlLiterals.size()) {
                expanded.append(sqlLiterals.get(next++));
            }
            else {
                expanded.append(c);
            }
        }
        return expanded.toString();
    }

    protected String quote(String name)
    {
        name = name.replace(DOUBLE_QUOTE_CHAR, DOUBLE_QUOTE_CHAR + DOUBLE_QUOTE_CHAR);
        return DOUBLE_QUOTE_CHAR + name + DOUBLE_QUOTE_CHAR;
    }

    protected String singleQuote(String name)
    {
        name = name.replace("\\", "\\\\").replace(SINGLE_QUOTE_CHAR, SINGLE_QUOTE_CHAR + SINGLE_QUOTE_CHAR);
        return SINGLE_QUOTE_CHAR + name + SINGLE_QUOTE_CHAR;
    }

    @Override
    protected String buildSQLStringLiteral(
            final String catalog,
            final String schema,
            final String table,
            final Schema tableSchema,
            final Constraints constraints,
            final Split split,
            final String columnNames,
            List<TypeAndValue> accumulator)
    {
        Constraints filterConstraints = new Constraints(constraints.getSummary(), constraints.getExpression(), Collections.emptyList(),
                Constraints.DEFAULT_NO_LIMIT, constraints.getQueryPassthroughArguments(), constraints.getQueryPlan());
        String sql = super.buildSQLStringLiteral(catalog, schema, table, tableSchema, filterConstraints, split, columnNames, accumulator)
                + appendOrderByLimitOffset(split, constraints);
        LOGGER.info("Generated Snowflake SQL : {}", sql);
        return sql;
    }

    @VisibleForTesting
    String appendOrderByLimitOffset(Split split, Constraints constraints)
    {
        String orderByClause = extractOrderByClause(constraints);
        Optional<SplitPage> page = getSplitPage(split);
        if (!page.isPresent()) {
            String sql = orderByClause.isEmpty() ? "" : " " + orderByClause;
            return constraints.getLimit() > 0 ? sql + appendLimitOffset(split, constraints) : sql;
        }
        String orderBy = orderByClause.isEmpty() ? " ORDER BY " : " " + orderByClause + ", ";
        return orderBy + page.get().primaryKey + appendLimitOffsetWithValue(page.get().limitWithin(constraints.getLimit()), page.get().offset);
    }

    @Override
    protected String getSplitPagingClause(Split split)
    {
        return getSplitPage(split)
                .map(page -> " ORDER BY " + page.primaryKey + appendLimitOffsetWithValue(page.limit, page.offset))
                .orElse("");
    }

    private Optional<SplitPage> getSplitPage(Split split)
    {
        String partitionVal = split == null ? null : split.getProperty(BLOCK_PARTITION_COLUMN_NAME);
        // Expected format: partition-primary-<PRIMARYKEY>-limit-<LIMIT>-offset-<OFFSET>
        // Use marker-based parsing to handle primary keys that may contain dashes
        if (partitionVal == null || !partitionVal.contains(PARTITION_PRIMARY_MARKER) || !partitionVal.contains(PARTITION_LIMIT_MARKER) || !partitionVal.contains(PARTITION_OFFSET_MARKER)) {
            return Optional.empty();
        }

        int primaryStart = partitionVal.indexOf(PARTITION_PRIMARY_MARKER) + PARTITION_PRIMARY_MARKER.length();
        int limitMarker = partitionVal.lastIndexOf(PARTITION_LIMIT_MARKER);
        int offsetMarker = partitionVal.lastIndexOf(PARTITION_OFFSET_MARKER);

        if (primaryStart > limitMarker || limitMarker > offsetMarker) {
            LOGGER.warn("Malformed partition value: {}", partitionVal);
            return Optional.empty();
        }

        String primaryKey = partitionVal.substring(primaryStart, limitMarker);
        String limit = partitionVal.substring(limitMarker + PARTITION_LIMIT_MARKER.length(), offsetMarker);
        String offset = partitionVal.substring(offsetMarker + PARTITION_OFFSET_MARKER.length());

        // if no primary key, single split only
        if (primaryKey.isEmpty()) {
            return Optional.empty();
        }

        // Validate limit and offset are numeric to prevent injection
        if (!limit.matches("\\d+") || !offset.matches("\\d+")) {
            LOGGER.warn("Non-numeric limit/offset in partition value: limit={}, offset={}", limit, offset);
            return Optional.empty();
        }

        // Primary key is already quoted from getPrimaryKey() (e.g., "\"col1\",\"col2\"")
        // so it is safe to use directly in ORDER BY
        return Optional.of(new SplitPage(primaryKey, limit, offset));
    }

    private static final class SplitPage
    {
        private final String primaryKey;
        private final String limit;
        private final String offset;

        private SplitPage(String primaryKey, String limit, String offset)
        {
            this.primaryKey = primaryKey;
            this.limit = limit;
            this.offset = offset;
        }

        private String limitWithin(long constraintLimit)
        {
            if (constraintLimit <= 0) {
                return limit;
            }
            try {
                return String.valueOf(Math.min(Long.parseLong(limit), constraintLimit));
            }
            catch (NumberFormatException e) {
                return String.valueOf(constraintLimit);
            }
        }
    }

    @Override
    protected SqlDialect getSqlDialect()
    {
        return new SnowflakeDialect(false);
    }
    
    @Override
    protected SqlDialect getSqlDialect(boolean catalogCasingFilterUpperCase)
    {
        return new SnowflakeDialect(catalogCasingFilterUpperCase);
    }
}
