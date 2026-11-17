import com.amazonaws.athena.connector.substrait.SubstraitAccumulatorVisitor;
import com.amazonaws.athena.connector.substrait.SubstraitSqlUtils;
import com.amazonaws.athena.connector.substrait.SubstraitTypeAndValue;
import com.amazonaws.athena.connector.util.EncodedSubstraitPlanStringGenerator;
import io.substrait.isthmus.calcite.SubstraitSchema;
import io.substrait.proto.Plan;
import org.apache.calcite.rel.type.RelDataType;
import org.apache.calcite.rel.type.RelDataTypeFactory;
import org.apache.calcite.schema.Schema;
import org.apache.calcite.schema.Table;
import org.apache.calcite.schema.impl.AbstractTable;
import org.apache.calcite.sql.SqlDialect;
import org.apache.calcite.sql.SqlNode;
import org.apache.calcite.sql.SqlSelect;
import org.apache.calcite.sql.dialect.AnsiSqlDialect;
import org.apache.calcite.sql.dialect.PostgresqlSqlDialect;
import org.apache.calcite.sql.dialect.SnowflakeSqlDialect;

import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.Base64;
import java.util.List;
import java.util.Locale;
import java.util.Map;

public class VerifyProbe
{
    static class UpperPg extends PostgresqlSqlDialect
    {
        UpperPg() { super(DEFAULT_CONTEXT); }
        @Override
        public StringBuilder quoteIdentifier(StringBuilder buf, String identifier)
        {
            String upper = identifier.toUpperCase(Locale.ROOT);
            return buf.append("\"").append(upper.replace("\"", "\"\"")).append("\"");
        }
    }

    static class UpperSf extends SnowflakeSqlDialect
    {
        UpperSf() { super(DEFAULT_CONTEXT); }
        @Override
        public StringBuilder quoteIdentifier(StringBuilder buf, String identifier)
        {
            String upper = identifier.toUpperCase(Locale.ROOT);
            return buf.append("\"").append(upper.replace("\"", "\"\"")).append("\"");
        }
    }

    static Table nullableTable()
    {
        Table base = EncodedSubstraitPlanStringGenerator.createTable();
        return new AbstractTable() {
            @Override
            public RelDataType getRowType(final RelDataTypeFactory factory)
            {
                RelDataType t = base.getRowType(factory);
                RelDataTypeFactory.Builder b = factory.builder();
                t.getFieldList().forEach(f -> b.add(f.getName(), factory.createTypeWithNullability(f.getType(), true)));
                return b.build();
            }
        };
    }

    static Table quotedTable()
    {
        return new AbstractTable() {
            @Override
            public RelDataType getRowType(final RelDataTypeFactory f)
            {
                RelDataTypeFactory.Builder b = f.builder();
                b.add("MixedCase", f.createTypeWithNullability(f.createSqlType(org.apache.calcite.sql.type.SqlTypeName.VARCHAR), true));
                b.add("col with space", f.createSqlType(org.apache.calcite.sql.type.SqlTypeName.INTEGER));
                b.add("select", f.createSqlType(org.apache.calcite.sql.type.SqlTypeName.INTEGER));
                b.add("order", f.createSqlType(org.apache.calcite.sql.type.SqlTypeName.VARCHAR));
                b.add("a\"b", f.createTypeWithNullability(f.createSqlType(org.apache.calcite.sql.type.SqlTypeName.BIGINT), true));
                b.add("Caf\u00e9", f.createSqlType(org.apache.calcite.sql.type.SqlTypeName.DATE));
                b.add("UPPER_COL", f.createSqlType(org.apache.calcite.sql.type.SqlTypeName.DECIMAL, 12, 3));
                return b.build();
            }
        };
    }

    static com.google.protobuf.Message fetchPatch(com.google.protobuf.Message m, String mode)
    {
        if (m instanceof io.substrait.proto.FetchRel) {
            io.substrait.proto.FetchRel f = (io.substrait.proto.FetchRel) m;
            System.err.println("ORIGINAL FETCH offset=" + f.getOffset() + " count=" + f.getCount() + " countMode=" + f.getCountModeCase() + " offsetMode=" + f.getOffsetModeCase());
            io.substrait.proto.FetchRel.Builder b = f.toBuilder();
            io.substrait.proto.Expression lit = io.substrait.proto.Expression.newBuilder().setLiteral(io.substrait.proto.Expression.Literal.newBuilder().setI64(10)).build();
            switch (mode) {
                case "countexpr": b.setCountExpr(lit); break;
                case "unset": b.clearCountMode(); break;
                case "offsetexpr": b.setOffsetExpr(io.substrait.proto.Expression.newBuilder().setLiteral(io.substrait.proto.Expression.Literal.newBuilder().setI64(5))); break;
                case "neg2": b.setCount(-2); break;
                default: throw new IllegalArgumentException(mode);
            }
            return b.build();
        }
        com.google.protobuf.Message.Builder b = m.toBuilder();
        for (Map.Entry<com.google.protobuf.Descriptors.FieldDescriptor, Object> e : m.getAllFields().entrySet()) {
            com.google.protobuf.Descriptors.FieldDescriptor fd = e.getKey();
            if (fd.getJavaType() == com.google.protobuf.Descriptors.FieldDescriptor.JavaType.MESSAGE) {
                if (fd.isRepeated()) {
                    List<?> l = (List<?>) e.getValue();
                    for (int i = 0; i < l.size(); i++) {
                        b.setRepeatedField(fd, i, fetchPatch((com.google.protobuf.Message) l.get(i), mode));
                    }
                }
                else {
                    b.setField(fd, fetchPatch((com.google.protobuf.Message) e.getValue(), mode));
                }
            }
        }
        return b.build();
    }

    static com.google.protobuf.Message patch(com.google.protobuf.Message m, String from, String to)
    {
        com.google.protobuf.Message.Builder b = m.toBuilder();
        for (Map.Entry<com.google.protobuf.Descriptors.FieldDescriptor, Object> e : m.getAllFields().entrySet()) {
            com.google.protobuf.Descriptors.FieldDescriptor fd = e.getKey();
            if (fd.getJavaType() == com.google.protobuf.Descriptors.FieldDescriptor.JavaType.MESSAGE) {
                if (fd.isRepeated()) {
                    List<?> l = (List<?>) e.getValue();
                    for (int i = 0; i < l.size(); i++) {
                        b.setRepeatedField(fd, i, patch((com.google.protobuf.Message) l.get(i), from, to));
                    }
                }
                else {
                    b.setField(fd, patch((com.google.protobuf.Message) e.getValue(), from, to));
                }
            }
            else if (fd.getJavaType() == com.google.protobuf.Descriptors.FieldDescriptor.JavaType.STRING && !fd.isRepeated() && from.equals(e.getValue())) {
                b.setField(fd, to);
            }
        }
        return b.build();
    }

    static String esc(String s)
    {
        return s.replace("\\", "\\\\").replace("\n", "\\n").replace("\t", "\\t");
    }

    public static void main(String[] args) throws Exception
    {
        List<String> lines = Files.readAllLines(Paths.get(args[0]));
        SqlDialect[] dialects = {AnsiSqlDialect.DEFAULT, PostgresqlSqlDialect.DEFAULT, SnowflakeSqlDialect.DEFAULT, new UpperPg(), new UpperSf()};
        String[] names = {"ansi", "postgres", "snowflake", "postgres_upper", "snowflake_upper"};
        StringBuilder out = new StringBuilder();
        for (String line : lines) {
            if (line.isBlank() || line.startsWith("#")) {
                continue;
            }
            String[] parts = line.split("\t", 3);
            String table = parts[0];
            final String q0 = parts[1];
            String q = q0;
            String patchSpec = parts.length > 2 ? parts[2] : null;
            Plan plan;
            java.util.concurrent.ExecutorService ex = java.util.concurrent.Executors.newSingleThreadExecutor(r -> { Thread t = new Thread(r); t.setDaemon(true); return t; });
            try {
                java.util.concurrent.Future<Plan> fut = ex.submit(() -> {
                    if (table.equals("quoted")) {
                        Schema schema = new SubstraitSchema(Map.of("Mixed Table", quotedTable()));
                        return EncodedSubstraitPlanStringGenerator.convertSqlToSubstraitPlan("MySchema", schema, q0);
                    }
                    if (table.equals("nullable")) {
                        Schema schema = new SubstraitSchema(Map.of("test_table", nullableTable()));
                        return EncodedSubstraitPlanStringGenerator.convertSqlToSubstraitPlan("test_schema", schema, q0);
                    }
                    return EncodedSubstraitPlanStringGenerator.convertSqlToSubstraitPlan(q0);
                });
                plan = fut.get(30, java.util.concurrent.TimeUnit.SECONDS);
            }
            catch (Throwable e) {
                System.err.println("SKIP " + q + " : " + e);
                ex.shutdownNow();
                continue;
            }
            ex.shutdown();
            if (patchSpec != null) {
                if (patchSpec.startsWith("FETCH:")) {
                    plan = (Plan) fetchPatch(plan, patchSpec.substring(6));
                    q = q + " [" + patchSpec + "]";
                    patchSpec = null;
                }
            }
            if (patchSpec != null) {
                String[] ft = patchSpec.split("=>", 2);
                plan = (Plan) patch(plan, ft[0], ft[1]);
                q = q + " [patched " + ft[0] + " => " + ft[1] + "]";
            }
            System.err.println("Q " + q);
            String b64 = Base64.getEncoder().encodeToString(plan.toByteArray());
            out.append("QUERY\t").append(table).append('\t').append(q).append('\n');
            out.append("PLAN\t").append(b64).append('\n');
            for (int i = 0; i < dialects.length; i++) {
                try {
                    SqlNode node = SubstraitSqlUtils.getSqlNodeFromSubstraitPlan(b64, dialects[i]);
                    out.append("CALCITE_").append(names[i]).append('\t').append(esc(node.toSqlString(dialects[i]).getSql())).append('\n');
                    if (node instanceof SqlSelect) {
                        try {
                            List<SubstraitTypeAndValue> acc = new ArrayList<>();
                            RelDataType schema = SubstraitSqlUtils.getTableSchemaFromSubstraitPlan(b64, dialects[i]);
                            SqlNode p = new SubstraitAccumulatorVisitor(acc, schema).visit((SqlSelect) node);
                            StringBuilder vals = new StringBuilder();
                            for (SubstraitTypeAndValue v : acc) {
                                vals.append('\t').append(v.getType()).append('|').append(v.getValue() == null ? "null" : v.getValue().getClass().getSimpleName()).append('|').append(esc(String.valueOf(v.getValue()))).append('|').append(v.getColumnName());
                            }
                            out.append("PARAM_").append(names[i]).append('\t').append(esc(p.toSqlString(dialects[i]).getSql())).append(vals).append('\n');
                        }
                        catch (Throwable e) {
                            out.append("PARAM_").append(names[i]).append("\tERROR ").append(esc(String.valueOf(e.getMessage()))).append('\n');
                        }
                    }
                }
                catch (Throwable e) {
                    out.append("CALCITE_").append(names[i]).append("\tERROR ").append(esc(String.valueOf(e.getMessage()))).append('\n');
                }
            }
            try {
                RelDataType schema = SubstraitSqlUtils.getTableSchemaFromSubstraitPlan(b64, AnsiSqlDialect.DEFAULT);
                out.append("SCHEMA\t").append(esc(schema.getFullTypeString())).append('\n');
            }
            catch (Throwable e) {
                out.append("SCHEMA\tERROR ").append(esc(String.valueOf(e.getMessage()))).append('\n');
            }
            try {
                Map<String, String> remap = SubstraitSqlUtils.getColumnRemapping(b64, AnsiSqlDialect.DEFAULT);
                StringBuilder r = new StringBuilder();
                for (Map.Entry<String, String> e : remap.entrySet()) {
                    r.append('\t').append(e.getKey()).append('=').append(e.getValue());
                }
                out.append("REMAP").append(r).append('\n');
            }
            catch (Throwable e) {
                out.append("REMAP\tERROR ").append(esc(String.valueOf(e.getMessage()))).append('\n');
            }
            Files.writeString(Paths.get(args[1]), out.toString());
        }
        Files.writeString(Paths.get(args[1]), out.toString());
    }
}
