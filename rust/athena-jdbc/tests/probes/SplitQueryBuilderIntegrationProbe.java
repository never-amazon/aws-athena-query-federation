import com.amazonaws.athena.connector.util.EncodedSubstraitPlanStringGenerator;
import com.amazonaws.athena.connectors.jdbc.manager.JdbcSplitQueryBuilderIntegrationTest;
import org.junit.jupiter.params.provider.Arguments;

import java.io.PrintWriter;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.sql.Connection;
import java.sql.ParameterMetaData;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.util.ArrayList;
import java.util.List;
import java.util.stream.Stream;

public class SplitQueryBuilderIntegrationProbe
{
    private static final String TEST_TABLE = "test_table";
    private static final String TEST_SCHEMA = "test_schema";
    private static final List<String> EVENTS = new ArrayList<>();

    static String escape(Object value)
    {
        String text = String.valueOf(value);
        return text.replace("\\", "\\\\").replace("\t", "\\t").replace("\n", "\\n").replace("\r", "\\r");
    }

    static void event(Object... fields)
    {
        StringBuilder line = new StringBuilder();
        for (int i = 0; i < fields.length; i++) {
            if (i > 0) {
                line.append('\t');
            }
            line.append(escape(fields[i]));
        }
        EVENTS.add(line.toString());
    }

    static String render(Object value)
    {
        if (value instanceof byte[]) {
            StringBuilder hex = new StringBuilder();
            for (byte b : (byte[]) value) {
                hex.append(String.format("%02x", b));
            }
            return hex.toString();
        }
        return String.valueOf(value);
    }

    static Object unwrap(Method method, Object target, Object[] args) throws Throwable
    {
        try {
            return method.invoke(target, args);
        }
        catch (InvocationTargetException e) {
            throw e.getCause();
        }
    }

    static ResultSet recordingResultSet(ResultSet real)
    {
        InvocationHandler handler = (proxy, method, args) -> {
            Object result = unwrap(method, real, args);
            String name = method.getName();
            if (name.equals("next")) {
                event("next", result);
            }
            else if (name.startsWith("get") && args != null && args.length == 1 && args[0] instanceof String) {
                event("get", name, args[0], render(result));
            }
            return result;
        };
        return (ResultSet) Proxy.newProxyInstance(ResultSet.class.getClassLoader(), new Class<?>[] {ResultSet.class}, handler);
    }

    static PreparedStatement recordingStatement(PreparedStatement real)
    {
        InvocationHandler handler = (proxy, method, args) -> {
            String name = method.getName();
            if (name.startsWith("set") && args != null && args.length == 2 && args[0] instanceof Integer) {
                event("bind", name, args[0], args[1] == null ? "null" : args[1].getClass().getName(), render(args[1]));
            }
            Object result = unwrap(method, real, args);
            if (name.equals("getParameterMetaData")) {
                event("parameter_count", ((ParameterMetaData) result).getParameterCount());
            }
            else if (name.equals("executeQuery")) {
                event("execute");
                return recordingResultSet((ResultSet) result);
            }
            return result;
        };
        return (PreparedStatement) Proxy.newProxyInstance(PreparedStatement.class.getClassLoader(), new Class<?>[] {PreparedStatement.class}, handler);
    }

    static Connection recordingConnection(Connection real)
    {
        InvocationHandler handler = (proxy, method, args) -> {
            if (method.getName().equals("prepareStatement") && args != null && args.length == 1) {
                event("prepare", args[0]);
                try {
                    return recordingStatement((PreparedStatement) unwrap(method, real, args));
                }
                catch (Throwable e) {
                    event("prepare_error", e.getClass().getName() + ": " + e.getMessage());
                    throw e;
                }
            }
            return unwrap(method, real, args);
        };
        return (Connection) Proxy.newProxyInstance(Connection.class.getClassLoader(), new Class<?>[] {Connection.class}, handler);
    }

    static void generate(String sql)
    {
        try {
            String plan = EncodedSubstraitPlanStringGenerator.generate(TEST_TABLE, TEST_SCHEMA, sql);
            String again = EncodedSubstraitPlanStringGenerator.generate(TEST_TABLE, TEST_SCHEMA, sql);
            if (!plan.equals(again)) {
                throw new IllegalStateException("plan generation is not deterministic for " + sql);
            }
            event("plan", sql, plan);
        }
        catch (Exception e) {
            event("generate_error", sql, e.getClass().getName() + ": " + e.getMessage());
        }
    }

    interface Body
    {
        void run(JdbcSplitQueryBuilderIntegrationTest test) throws Exception;
    }

    static void run(String method, Body body, Object... label) throws Exception
    {
        JdbcSplitQueryBuilderIntegrationTest test = new JdbcSplitQueryBuilderIntegrationTest();
        test.setup();
        Field field = JdbcSplitQueryBuilderIntegrationTest.class.getDeclaredField("realConnection");
        field.setAccessible(true);
        Connection real = (Connection) field.get(test);
        field.set(test, recordingConnection(real));
        Object[] fields = new Object[label.length + 2];
        fields[0] = "test";
        fields[1] = method;
        System.arraycopy(label, 0, fields, 2, label.length);
        event(fields);
        try {
            body.run(test);
            event("result", "passed");
        }
        catch (Throwable e) {
            event("result", "failed", e.getClass().getName() + ": " + e.getMessage());
        }
        field.set(test, real);
        test.tearDown();
        event("end");
    }

    public static void main(String[] argv) throws Exception
    {
        run("testRealPreparedStatementWithMultipleParameters", test -> test.testRealPreparedStatementWithMultipleParameters());

        generate("SELECT * FROM test_table");
        run("testRealPreparedStatementWithCalciteSql", test -> test.testRealPreparedStatementWithCalciteSql());

        Method provider = JdbcSplitQueryBuilderIntegrationTest.class.getDeclaredMethod("provideCalciteSqlTestCases");
        provider.setAccessible(true);
        @SuppressWarnings("unchecked")
        Stream<Arguments> cases = (Stream<Arguments>) provider.invoke(null);
        for (Arguments arguments : (Iterable<Arguments>) cases::iterator) {
            Object[] a = arguments.get();
            String category = (String) a[0];
            String name = (String) a[1];
            String sql = (String) a[2];
            int expected = (Integer) a[3];
            generate(sql);
            run("testRealPreparedStatementWithCalciteSqlParameterized",
                    test -> test.testRealPreparedStatementWithCalciteSqlParameterized(category, name, sql, expected),
                    category, name, sql, expected);
        }

        generate("SELECT * FORM test_table");
        run("testRealPreparedStatementWithCalciteSqlErrorCases", test -> test.testRealPreparedStatementWithCalciteSqlErrorCases());

        generate("SELECT invalid_column FROM test_table");
        run("testRealPreparedStatementWithCalciteSqlInvalidColumn", test -> test.testRealPreparedStatementWithCalciteSqlInvalidColumn());

        try (PrintWriter out = new PrintWriter(Files.newBufferedWriter(Paths.get(argv[0]), StandardCharsets.UTF_8))) {
            for (String line : EVENTS) {
                out.print(line);
                out.print('\n');
            }
        }
    }
}
