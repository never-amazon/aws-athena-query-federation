package com.amazonaws.athena.parity.wire;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ArrayNode;
import com.fasterxml.jackson.databind.node.JsonNodeFactory;
import com.fasterxml.jackson.databind.node.ObjectNode;
import org.apache.arrow.memory.BufferAllocator;
import org.apache.arrow.memory.RootAllocator;
import org.apache.arrow.vector.FieldVector;
import org.apache.arrow.vector.VectorLoader;
import org.apache.arrow.vector.VectorSchemaRoot;
import org.apache.arrow.vector.ipc.ReadChannel;
import org.apache.arrow.vector.ipc.message.ArrowRecordBatch;
import org.apache.arrow.vector.ipc.message.MessageSerializer;
import org.apache.arrow.vector.types.pojo.Field;
import org.apache.arrow.vector.types.pojo.Schema;
import org.apache.arrow.vector.util.Text;

import java.io.BufferedReader;
import java.io.ByteArrayInputStream;
import java.io.InputStreamReader;
import java.io.PrintStream;
import java.math.BigDecimal;
import java.nio.channels.Channels;
import java.nio.charset.StandardCharsets;
import java.time.LocalDateTime;
import java.util.Base64;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

import javax.crypto.Cipher;
import javax.crypto.spec.GCMParameterSpec;
import javax.crypto.spec.SecretKeySpec;

public final class ArrowDecode
{
    private static final ObjectMapper JSON = new ObjectMapper();
    private static final JsonNodeFactory NODES = JsonNodeFactory.instance;

    private ArrowDecode() {}

    public static void main(String[] args) throws Exception
    {
        PrintStream out = new PrintStream(System.out, false, StandardCharsets.UTF_8);
        try (BufferAllocator allocator = new RootAllocator();
                BufferedReader in = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8))) {
            String line;
            while ((line = in.readLine()) != null) {
                if (line.isBlank()) {
                    continue;
                }
                JsonNode request = JSON.readTree(line);
                ObjectNode result = NODES.objectNode();
                try {
                    decode(allocator, request, result);
                }
                catch (Exception e) {
                    result.removeAll();
                    result.put("decodeError", e.getClass().getSimpleName() + ": " + e.getMessage());
                }
                out.println(JSON.writeValueAsString(result));
            }
        }
        out.flush();
    }

    private static void decode(BufferAllocator allocator, JsonNode request, ObjectNode result) throws Exception
    {
        Schema schema = MessageSerializer.deserializeSchema(channel(request.get("schema").asText()));
        result.set("schema", schemaJson(schema));
        if (schema.getCustomMetadata() != null && !schema.getCustomMetadata().isEmpty()) {
            ObjectNode metadata = result.putObject("schemaMetadata");
            new TreeMap<>(schema.getCustomMetadata()).forEach(metadata::put);
        }
        JsonNode records = request.get("records");
        if (records == null || records.isNull()) {
            return;
        }
        ArrayNode rows = result.putArray("rows");
        if (records.asText().isEmpty()) {
            result.put("rowCount", 0);
            return;
        }
        byte[] recordBytes = Base64.getDecoder().decode(records.asText());
        JsonNode key = request.get("key");
        if (key != null && !key.isNull()) {
            Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
            cipher.init(Cipher.DECRYPT_MODE, new SecretKeySpec(Base64.getDecoder().decode(key.asText()), "AES"),
                    new GCMParameterSpec(128, Base64.getDecoder().decode(request.get("nonce").asText())));
            recordBytes = cipher.doFinal(recordBytes);
            result.put("decrypted", true);
        }
        try (ArrowRecordBatch batch = MessageSerializer.deserializeRecordBatch(channel(recordBytes), allocator);
                VectorSchemaRoot root = VectorSchemaRoot.create(schema, allocator)) {
            new VectorLoader(root).load(batch);
            result.put("rowCount", root.getRowCount());
            for (int row = 0; row < root.getRowCount(); row++) {
                ArrayNode values = rows.addArray();
                for (FieldVector vector : root.getFieldVectors()) {
                    values.add(value(vector.getObject(row)));
                }
            }
        }
    }

    private static ReadChannel channel(String base64)
    {
        return channel(Base64.getDecoder().decode(base64));
    }

    private static ReadChannel channel(byte[] bytes)
    {
        return new ReadChannel(Channels.newChannel(new ByteArrayInputStream(bytes)));
    }

    private static ArrayNode schemaJson(Schema schema)
    {
        ArrayNode fields = NODES.arrayNode();
        for (Field field : schema.getFields()) {
            fields.add(fieldJson(field));
        }
        return fields;
    }

    private static ObjectNode fieldJson(Field field)
    {
        ObjectNode node = NODES.objectNode();
        node.put("name", field.getName());
        node.put("type", field.getType().toString());
        node.put("nullable", field.isNullable());
        if (field.getDictionary() != null) {
            node.put("dictionary", field.getDictionary().toString());
        }
        if (field.getMetadata() != null && !field.getMetadata().isEmpty()) {
            ObjectNode metadata = node.putObject("metadata");
            new TreeMap<>(field.getMetadata()).forEach(metadata::put);
        }
        if (!field.getChildren().isEmpty()) {
            ArrayNode children = node.putArray("children");
            for (Field child : field.getChildren()) {
                children.add(fieldJson(child));
            }
        }
        return node;
    }

    private static JsonNode value(Object value)
    {
        if (value == null) {
            return NODES.nullNode();
        }
        if (value instanceof Boolean) {
            return NODES.booleanNode((Boolean) value);
        }
        if (value instanceof Byte || value instanceof Short || value instanceof Integer || value instanceof Long) {
            return NODES.numberNode(((Number) value).longValue());
        }
        if (value instanceof Float) {
            return NODES.textNode("f32:" + value);
        }
        if (value instanceof Double) {
            return NODES.textNode("f64:" + value);
        }
        if (value instanceof BigDecimal) {
            return NODES.textNode("dec:" + ((BigDecimal) value).toPlainString());
        }
        if (value instanceof Text) {
            return NODES.textNode(value.toString());
        }
        if (value instanceof byte[]) {
            return NODES.textNode("b64:" + Base64.getEncoder().encodeToString((byte[]) value));
        }
        if (value instanceof LocalDateTime) {
            return NODES.textNode("ts:" + value);
        }
        if (value instanceof List) {
            ArrayNode list = NODES.arrayNode();
            for (Object item : (List<?>) value) {
                list.add(value(item));
            }
            return list;
        }
        if (value instanceof Map) {
            ObjectNode map = NODES.objectNode();
            for (Map.Entry<?, ?> entry : new TreeMap<>((Map<?, ?>) value).entrySet()) {
                map.set(String.valueOf(entry.getKey()), value(entry.getValue()));
            }
            return map;
        }
        return NODES.textNode(value.getClass().getSimpleName() + ":" + value);
    }
}
