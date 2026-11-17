package com.amazonaws.athena.parity.wire;

import com.amazonaws.athena.connector.lambda.data.SchemaBuilder;
import org.apache.arrow.vector.types.Types;
import org.apache.arrow.vector.types.pojo.ArrowType;
import org.apache.arrow.vector.types.pojo.Field;
import org.apache.arrow.vector.types.pojo.Schema;

import java.util.ArrayList;
import java.util.List;

final class Schemas
{
    private static final ArrowType INT = Types.MinorType.INT.getType();
    private static final ArrowType BIGINT = Types.MinorType.BIGINT.getType();
    private static final ArrowType SMALLINT = Types.MinorType.SMALLINT.getType();
    private static final ArrowType FLOAT4 = Types.MinorType.FLOAT4.getType();
    private static final ArrowType FLOAT8 = Types.MinorType.FLOAT8.getType();
    private static final ArrowType VARCHAR = Types.MinorType.VARCHAR.getType();
    private static final ArrowType BIT = Types.MinorType.BIT.getType();
    private static final ArrowType DATEDAY = Types.MinorType.DATEDAY.getType();
    private static final ArrowType DATEMILLI = Types.MinorType.DATEMILLI.getType();
    private static final ArrowType VARBINARY = Types.MinorType.VARBINARY.getType();

    private Schemas() {}

    private static ArrowType decimal(int precision, int scale)
    {
        return new ArrowType.Decimal(precision, scale, 128);
    }

    private static SchemaBuilder withPartitionCols(SchemaBuilder builder)
    {
        return builder.addField("partition_schema_name", VARCHAR).addField("partition_name", VARCHAR);
    }

    static Schema allTypes()
    {
        return withPartitionCols(SchemaBuilder.newBuilder()
                .addField("id", INT)
                .addField("smallint_col", SMALLINT)
                .addField("int_col", INT)
                .addField("bigint_col", BIGINT)
                .addField("numeric_scaled", decimal(18, 4))
                .addField("numeric_noscale", decimal(10, 0))
                .addField("numeric_unbounded", decimal(38, 0))
                .addField("real_col", FLOAT4)
                .addField("double_col", FLOAT8)
                .addField("text_col", VARCHAR)
                .addField("varchar_col", VARCHAR)
                .addField("char_col", VARCHAR)
                .addField("bool_col", BIT)
                .addField("date_col", DATEDAY)
                .addField("ts_col", DATEMILLI)
                .addField("tstz_col", DATEMILLI)
                .addField("bytea_col", VARBINARY)
                .addListField("int_arr", INT)
                .addListField("bigint_arr", BIGINT)
                .addListField("text_arr", VARCHAR)
                .addListField("float8_arr", FLOAT8)
                .addListField("bool_arr", BIT)
                .addListField("date_arr", DATEDAY)
                .addListField("numeric_arr", decimal(10, 2))
                .addField("uuid_col", VARCHAR)
                .addField("json_col", VARCHAR)
                .addField("time_col", VARCHAR))
                .build();
    }

    static Schema sales()
    {
        return SchemaBuilder.newBuilder()
                .addField("sale_id", INT)
                .addField("region", VARCHAR)
                .addField("sale_date", DATEDAY)
                .addField("amount", decimal(12, 2))
                .addField("note", VARCHAR)
                .build();
    }

    static Schema salesWithPartitionCols()
    {
        return withPartitionCols(SchemaBuilder.newBuilder()
                .addField("sale_id", INT)
                .addField("region", VARCHAR)
                .addField("sale_date", DATEDAY)
                .addField("amount", decimal(12, 2))
                .addField("note", VARCHAR))
                .build();
    }

    static Schema view()
    {
        return withPartitionCols(SchemaBuilder.newBuilder()
                .addField("id", INT)
                .addField("int_col", INT)
                .addField("text_col", VARCHAR)
                .addField("date_col", DATEDAY)
                .addField("numeric_scaled", decimal(18, 4)))
                .build();
    }

    static Schema matView()
    {
        return withPartitionCols(SchemaBuilder.newBuilder()
                .addField("region", VARCHAR)
                .addField("sale_count", BIGINT)
                .addField("total_amount", decimal(38, 0)))
                .build();
    }

    static Schema camelTable()
    {
        return withPartitionCols(SchemaBuilder.newBuilder()
                .addField("Id", INT)
                .addField("CamelValue", VARCHAR)
                .addField("lower_value", INT))
                .build();
    }

    static Schema mixedTable()
    {
        return withPartitionCols(SchemaBuilder.newBuilder()
                .addField("MixedId", INT)
                .addField("MixedText", VARCHAR)
                .addField("UPPER_COL", FLOAT8))
                .build();
    }

    static Schema smallPk()
    {
        return withPartitionCols(SchemaBuilder.newBuilder()
                .addField("id", INT)
                .addField("label", VARCHAR))
                .build();
    }

    static Schema uuidPk()
    {
        return withPartitionCols(SchemaBuilder.newBuilder()
                .addField("id", VARCHAR)
                .addField("payload", VARCHAR))
                .build();
    }

    static Schema emptyTable()
    {
        return withPartitionCols(SchemaBuilder.newBuilder()
                .addField("id", INT)
                .addField("v", VARCHAR))
                .build();
    }

    static Schema qptScalar()
    {
        return SchemaBuilder.newBuilder()
                .addField("id", INT)
                .addField("smallint_col", SMALLINT)
                .addField("bigint_col", BIGINT)
                .addField("numeric_scaled", decimal(18, 4))
                .addField("real_col", FLOAT4)
                .addField("double_col", FLOAT8)
                .addField("text_col", VARCHAR)
                .addField("char_col", VARCHAR)
                .addField("bool_col", BIT)
                .addField("date_col", DATEDAY)
                .addField("ts_col", DATEMILLI)
                .addField("bytea_col", VARBINARY)
                .build();
    }

    static Schema project(Schema schema, String... names)
    {
        List<Field> fields = new ArrayList<>();
        for (String name : names) {
            fields.add(schema.findField(name));
        }
        return new Schema(fields);
    }
}
