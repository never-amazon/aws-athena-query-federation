package com.amazonaws.athena.connector.lambda.data;

/*-
 * #%L
 * Amazon Athena Query Federation SDK
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

import com.amazonaws.athena.connector.lambda.exceptions.AthenaConnectorException;
import com.google.common.collect.ImmutableList;
import org.apache.arrow.vector.BigIntVector;
import org.apache.arrow.vector.DecimalVector;
import org.apache.arrow.vector.FieldVector;
import org.apache.arrow.vector.Float8Vector;
import org.apache.arrow.vector.IntVector;
import org.apache.arrow.vector.SmallIntVector;
import org.apache.arrow.vector.TinyIntVector;
import org.apache.arrow.vector.types.DateUnit;
import org.apache.arrow.vector.types.FloatingPointPrecision;
import org.apache.arrow.vector.types.TimeUnit;
import org.apache.arrow.vector.types.Types;
import org.apache.arrow.vector.types.pojo.ArrowType;
import org.apache.arrow.vector.types.pojo.Schema;
import org.junit.After;
import org.junit.Before;
import org.junit.Rule;
import org.junit.Test;
import org.junit.rules.TestName;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.math.BigDecimal;
import java.math.BigInteger;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.time.ZoneId;
import java.time.ZonedDateTime;
import java.util.Date;
import java.util.TimeZone;

import static org.junit.Assert.*;

public class BlockUtilsTest
{
    private static final Logger logger = LoggerFactory.getLogger(BlockUtilsTest.class);

    private BlockAllocatorImpl allocator;

    @Rule
    public TestName testName = new TestName();

    @Before
    public void setup()
    {
        logger.info("{}: enter", testName.getMethodName());
        // This is the default behavior of DateTimeFormatterUtil but some tests
        // will selectively disable it.
        DateTimeFormatterUtil.enableTimezonePacking();
        TimeZone.setDefault(TimeZone.getTimeZone("UTC"));
        allocator = new BlockAllocatorImpl();
    }

    @After
    public void after()
    {
        allocator.close();
        logger.info("{}: exit ", testName.getMethodName());
    }

    @Test
    public void copyRows()
    {
        for (TimeUnit timeUnit : ImmutableList.of(TimeUnit.MILLISECOND, TimeUnit.MICROSECOND)) {
            if (timeUnit.equals(TimeUnit.MICROSECOND)) {
                DateTimeFormatterUtil.disableTimezonePacking();
            }
            Schema schema = SchemaBuilder.newBuilder()
                    .addField("col1", new ArrowType.Int(32, true))

                    .addField("col2", new ArrowType.Decimal(38, 9))
                    .addField("col3", new ArrowType.Timestamp(timeUnit, "UTC"))
                    .build();

            LocalDateTime ldt = LocalDateTime.of(2020, 03, 18, 12,54,29);
            Date date = Date.from(ldt.atZone(ZoneId.systemDefault()).toInstant());

            //Make a block with 3 rows
            Block src = allocator.createBlock(schema);
            BlockUtils.setValue(src.getFieldVector("col1"), 0, 10);
            BlockUtils.setValue(src.getFieldVector("col2"), 0, new BigDecimal(20));
            BlockUtils.setValue(src.getFieldVector("col3"), 0, ldt);

            BlockUtils.setValue(src.getFieldVector("col1"), 1, 11);
            BlockUtils.setValue(src.getFieldVector("col2"), 1, new BigDecimal(21));
            BlockUtils.setValue(src.getFieldVector("col3"), 1, ZonedDateTime.of(ldt, ZoneId.of("-05:00")));

            BlockUtils.setValue(src.getFieldVector("col1"), 2, 12);
            BlockUtils.setValue(src.getFieldVector("col2"), 2, new BigDecimal(22));
            BlockUtils.setValue(src.getFieldVector("col3"), 2, date);
            src.setRowCount(3);

            //Make the destination block
            Block dst = allocator.createBlock(schema);

            assertEquals(3, BlockUtils.copyRows(src, dst, 0, 2));

            assertEquals(src, dst);
        }
    }

    @Test
    public void isNullRow()
    {
        for (TimeUnit timeUnit : ImmutableList.of(TimeUnit.MILLISECOND, TimeUnit.MICROSECOND)) {
            if (timeUnit.equals(TimeUnit.MICROSECOND)) {
                DateTimeFormatterUtil.disableTimezonePacking();
            }
            Schema schema = SchemaBuilder.newBuilder()
                    .addField("col1", new ArrowType.Int(32, true))
                    .addField("col2", new ArrowType.Int(32, true))
                    .addField("col3", new ArrowType.Timestamp(timeUnit, "UTC"))
                    .build();

            LocalDateTime ldt = LocalDateTime.now();

            //Make a block with 2 rows and no null rows
            Block block = allocator.createBlock(schema);
            BlockUtils.setValue(block.getFieldVector("col1"), 0, 10);
            BlockUtils.setValue(block.getFieldVector("col2"), 0, 20);
            BlockUtils.setValue(block.getFieldVector("col3"), 0, ldt);

            BlockUtils.setValue(block.getFieldVector("col1"), 1, 11);
            BlockUtils.setValue(block.getFieldVector("col2"), 1, 21);
            BlockUtils.setValue(block.getFieldVector("col3"), 1, ZonedDateTime.of(ldt, ZoneId.of("-05:00")));
            block.setRowCount(2);

            assertFalse(BlockUtils.isNullRow(block, 1));

            //now set a row to null
            BlockUtils.unsetRow(1, block);
            assertTrue(BlockUtils.isNullRow(block, 1));
        }
    }

    @Test
    public void fieldToString() throws java.text.ParseException
    {
        for (TimeUnit timeUnit : ImmutableList.of(TimeUnit.MILLISECOND, TimeUnit.MICROSECOND)) {
            String expectedString = "Block{rows=2, col1=[10, 11], col2=[2020-03-18T12:54:29Z[UTC], 2020-03-18T12:54:29-05:00], col3=[2020-03-18, 2019-12-29]}";
            if (timeUnit.equals(TimeUnit.MICROSECOND)) {
                DateTimeFormatterUtil.disableTimezonePacking();
                // The difference here is because TimeUnit.MILLISECOND with packing is using the packed
                // timezone value rather than using the timezone from the ArrowType
                expectedString = "Block{rows=2, col1=[10, 11], col2=[2020-03-18T12:54:29Z[UTC], 2020-03-18T17:54:29Z[UTC]], col3=[2020-03-18, 2019-12-29]}";
            }

            Schema schema = SchemaBuilder.newBuilder()
                    .addField("col1", new ArrowType.Int(32, true))
                    .addField("col2", new ArrowType.Timestamp(timeUnit, "UTC"))
                    .addField("col3", new ArrowType.Date(org.apache.arrow.vector.types.DateUnit.DAY))
                    .build();

            LocalDateTime ldt = LocalDateTime.of(2020, 03, 18, 12,54,29);

            //Make a block with 2 rows and no null rows
            Block block = allocator.createBlock(schema);
            BlockUtils.setValue(block.getFieldVector("col1"), 0, 10);
            BlockUtils.setValue(block.getFieldVector("col2"), 0, ldt);
            BlockUtils.setValue(block.getFieldVector("col3"), 0, java.sql.Timestamp.valueOf(ldt));

            BlockUtils.setValue(block.getFieldVector("col1"), 1, 11);
            BlockUtils.setValue(block.getFieldVector("col2"), 1, ZonedDateTime.of(ldt, ZoneId.of("-05:00")));
            BlockUtils.setValue(block.getFieldVector("col3"), 1, new java.text.SimpleDateFormat("yyyy-MM-dd").parse("2019-12-29"));
            block.setRowCount(2);

            String actual = block.toString();

            assertEquals(expectedString, actual);
        }
    }

    @Test
    public void canSetDate()
    {
        Schema schema = SchemaBuilder.newBuilder()
                .addField("col1", new ArrowType.Date(DateUnit.DAY))
                .build();

        LocalDate date = LocalDate.parse("1998-01-01");

        Block block = allocator.createBlock(schema);
        BlockUtils.setValue(block.getFieldVector("col1"), 0, date);
    }

    @Test
    public void setValueConvertsOtherNumbersExactly()
    {
        Schema schema = SchemaBuilder.newBuilder()
                .addField("int", new ArrowType.Int(32, true))
                .addField("bigint", new ArrowType.Int(64, true))
                .addField("smallint", new ArrowType.Int(16, true))
                .addField("tinyint", new ArrowType.Int(8, true))
                .addField("float8", new ArrowType.FloatingPoint(FloatingPointPrecision.DOUBLE))
                .addField("decimal", new ArrowType.Decimal(10, 2))
                .build();
        Block block = allocator.createBlock(schema);

        BlockUtils.setValue(block.getFieldVector("int"), 0, new BigDecimal("42"));
        BlockUtils.setValue(block.getFieldVector("bigint"), 0, new BigDecimal("9223372036854775807"));
        BlockUtils.setValue(block.getFieldVector("smallint"), 0, new BigDecimal("-300.00"));
        BlockUtils.setValue(block.getFieldVector("tinyint"), 0, BigInteger.valueOf(-128));
        BlockUtils.setValue(block.getFieldVector("float8"), 0, new BigDecimal("1.25"));
        BlockUtils.setValue(block.getFieldVector("decimal"), 0, 7L);

        assertEquals(42, ((IntVector) block.getFieldVector("int")).get(0));
        assertEquals(Long.MAX_VALUE, ((BigIntVector) block.getFieldVector("bigint")).get(0));
        assertEquals(-300, ((SmallIntVector) block.getFieldVector("smallint")).get(0));
        assertEquals(-128, ((TinyIntVector) block.getFieldVector("tinyint")).get(0));
        assertEquals(1.25, ((Float8Vector) block.getFieldVector("float8")).get(0), 0.0);
        assertEquals(new BigDecimal("7.00"), ((DecimalVector) block.getFieldVector("decimal")).getObject(0));
    }

    @Test
    public void setValueRejectsLossyIntegerConversions()
    {
        Schema schema = SchemaBuilder.newBuilder()
                .addField("int", new ArrowType.Int(32, true))
                .addField("bigint", new ArrowType.Int(64, true))
                .addField("tinyint", new ArrowType.Int(8, true))
                .build();
        Block block = allocator.createBlock(schema);
        FieldVector intVector = block.getFieldVector("int");
        FieldVector bigintVector = block.getFieldVector("bigint");
        FieldVector tinyintVector = block.getFieldVector("tinyint");
        // 2^64 - 1, which drivers return as a BigInteger for an unsigned 64-bit column.
        BigInteger maxUnsignedLong = BigInteger.ONE.shiftLeft(64).subtract(BigInteger.ONE);

        assertThrows(AthenaConnectorException.class, () -> BlockUtils.setValue(intVector, 0, new BigDecimal("12.7")));
        assertThrows(AthenaConnectorException.class, () -> BlockUtils.setValue(intVector, 0, new BigDecimal("3000000000")));
        assertThrows(AthenaConnectorException.class, () -> BlockUtils.setValue(bigintVector, 0, new BigDecimal("1.9")));
        assertThrows(AthenaConnectorException.class, () -> BlockUtils.setValue(bigintVector, 0, new BigDecimal("9223372036854775808")));
        assertThrows(AthenaConnectorException.class, () -> BlockUtils.setValue(bigintVector, 0, maxUnsignedLong));
        assertThrows(AthenaConnectorException.class, () -> BlockUtils.setValue(bigintVector, 0, 1.9d));
        assertThrows(AthenaConnectorException.class, () -> BlockUtils.setValue(tinyintVector, 0, new BigDecimal("300")));
    }

    @Test
    public void setValueRoundsDoubleIntoDecimalFromItsDecimalForm()
    {
        Schema schema = SchemaBuilder.newBuilder()
                .addField("decimal", new ArrowType.Decimal(10, 2))
                .build();
        Block block = allocator.createBlock(schema);

        // new BigDecimal(2.675) is 2.67499..., which would round down to 2.67.
        BlockUtils.setValue(block.getFieldVector("decimal"), 0, 2.675d);

        assertEquals(new BigDecimal("2.68"), ((DecimalVector) block.getFieldVector("decimal")).getObject(0));
    }
}
