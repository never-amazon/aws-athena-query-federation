package com.amazonaws.athena.parity.wire;

import com.amazonaws.athena.connector.lambda.exceptions.AthenaConnectorException;
import com.amazonaws.athena.connectors.postgresql.ParityHandlers;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.SerializationFeature;
import com.fasterxml.jackson.databind.node.ArrayNode;
import com.fasterxml.jackson.databind.node.ObjectNode;

import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.Base64;
import java.util.HashMap;
import java.util.Iterator;
import java.util.List;
import java.util.Map;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.stream.Collectors;
import java.util.stream.Stream;

public final class JavaRunner
{
    private static final ObjectMapper JSON = new ObjectMapper().enable(SerializationFeature.INDENT_OUTPUT)
            .enable(SerializationFeature.ORDER_MAP_ENTRIES_BY_KEYS);

    static final String SECRET_NAME = "parity-pg-secret";
    private static final Pattern PLACEHOLDER = Pattern.compile("\\{\\{([A-Za-z0-9_]+)}}");

    private JavaRunner() {}

    public static void main(String[] args) throws Exception
    {
        if (args.length < 2) {
            System.err.println("usage: JavaRunner <corpus dir> <out dir> [case name filter]");
            System.exit(2);
        }
        Path corpus = Paths.get(args[0]);
        Path out = Paths.get(args[1]);
        String filter = args.length > 2 ? args[2] : null;
        List<Path> cases;
        try (Stream<Path> stream = Files.list(corpus)) {
            cases = stream.filter(Files::isDirectory)
                    .filter(p -> filter == null || p.getFileName().toString().contains(filter))
                    .sorted()
                    .collect(Collectors.toList());
        }
        Files.createDirectories(out);
        int errors = 0;
        for (Path caseDir : cases) {
            if (!runCase(caseDir, out.resolve(caseDir.getFileName().toString()))) {
                errors++;
            }
        }
        System.out.println("cases=" + cases.size() + " errors=" + errors);
    }

    private static boolean runCase(Path caseDir, Path outDir) throws IOException
    {
        String name = caseDir.getFileName().toString();
        byte[] request = Files.readAllBytes(caseDir.resolve("request.json"));
        JsonNode meta = JSON.readTree(caseDir.resolve("case.json").toFile());
        Map<String, String> config = new HashMap<>(System.getenv());
        JsonNode env = meta.path("env");
        for (Iterator<Map.Entry<String, JsonNode>> it = env.fields(); it.hasNext(); ) {
            Map.Entry<String, JsonNode> entry = it.next();
            if (entry.getValue().isNull()) {
                config.remove(entry.getKey());
            }
            else {
                config.put(entry.getKey(), expand(entry.getValue().asText()));
            }
        }
        String mode = meta.path("handler").asText("single");

        Files.createDirectories(outDir);
        Files.deleteIfExists(outDir.resolve("java-response.json"));
        Files.deleteIfExists(outDir.resolve("java-error.txt"));
        Files.deleteIfExists(outDir.resolve("java-aws.json"));
        Files.write(outDir.resolve("request.json"), request);
        Files.copy(caseDir.resolve("case.json"), outDir.resolve("case.json"), java.nio.file.StandardCopyOption.REPLACE_EXISTING);

        Stubs stubs = new Stubs(secrets());
        ByteArrayOutputStream response = new ByteArrayOutputStream();
        Throwable failure = null;
        try (ParityHandlers handlers = ParityHandlers.create(mode, config, stubs.s3(), stubs.secretsManager(), stubs.athena())) {
            handlers.composite().handleRequest(new ByteArrayInputStream(request), response, Stubs.context("parity-" + name));
        }
        catch (Throwable t) {
            failure = t;
        }

        if (failure == null) {
            Files.write(outDir.resolve("java-response.json"), response.toByteArray());
        }
        else {
            Files.write(outDir.resolve("java-error.txt"), describe(failure).getBytes(StandardCharsets.UTF_8));
        }

        ObjectNode aws = JSON.createObjectNode();
        ArrayNode calls = aws.putArray("calls");
        stubs.calls().forEach(calls::add);
        ArrayNode objects = aws.putArray("s3Objects");
        for (Map.Entry<String, byte[]> entry : stubs.s3Objects().entrySet()) {
            ObjectNode object = objects.addObject();
            object.put("path", entry.getKey());
            object.put("size", entry.getValue().length);
            object.put("bytes", Base64.getEncoder().encodeToString(entry.getValue()));
        }
        if (calls.size() > 0 || objects.size() > 0) {
            Files.write(outDir.resolve("java-aws.json"), JSON.writeValueAsBytes(aws));
        }
        System.out.println((failure == null ? "ok    " : "error ") + name);
        return failure == null;
    }

    static String expand(String value)
    {
        Matcher matcher = PLACEHOLDER.matcher(value);
        StringBuilder result = new StringBuilder();
        while (matcher.find()) {
            String replacement = System.getenv(matcher.group(1));
            if (replacement == null) {
                throw new IllegalStateException("case env refers to unset variable " + matcher.group(1));
            }
            matcher.appendReplacement(result, Matcher.quoteReplacement(replacement));
        }
        matcher.appendTail(result);
        return result.toString();
    }

    private static Map<String, String> secrets() throws IOException
    {
        String password = System.getenv("PG_PASSWORD");
        if (password == null) {
            return Map.of();
        }
        ObjectNode secret = JSON.createObjectNode();
        secret.put("username", "parity");
        secret.put("password", password);
        return Map.of(SECRET_NAME, new ObjectMapper().writeValueAsString(secret));
    }

    static String describe(Throwable failure)
    {
        StringBuilder text = new StringBuilder();
        text.append("errorType: ").append(failure.getClass().getName()).append('\n');
        text.append("errorMessage: ").append(oneLine(failure.getMessage())).append('\n');
        if (failure instanceof AthenaConnectorException && ((AthenaConnectorException) failure).getErrorDetails() != null) {
            text.append("errorCode: ").append(((AthenaConnectorException) failure).getErrorDetails().errorCode()).append('\n');
        }
        List<Throwable> seen = new ArrayList<>();
        seen.add(failure);
        Throwable cause = failure.getCause();
        while (cause != null && !seen.contains(cause)) {
            text.append("cause: ").append(cause.getClass().getName()).append(": ").append(oneLine(cause.getMessage())).append('\n');
            seen.add(cause);
            cause = cause.getCause();
        }
        return text.toString();
    }

    private static String oneLine(String message)
    {
        return message == null ? "null" : message.replace("\r", "\\r").replace("\n", "\\n");
    }
}
