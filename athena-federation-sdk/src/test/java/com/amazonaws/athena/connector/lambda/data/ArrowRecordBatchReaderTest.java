/*-
 * #%L
 * Amazon Athena Query Federation SDK
 * %%
 * Copyright (C) 2026 Amazon Web Services
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
package com.amazonaws.athena.connector.lambda.data;

import com.google.flatbuffers.FlatBufferBuilder;
import org.apache.arrow.flatbuf.MessageHeader;
import org.apache.arrow.memory.ArrowBuf;
import org.apache.arrow.memory.BufferAllocator;
import org.apache.arrow.memory.RootAllocator;
import org.apache.arrow.vector.ipc.WriteChannel;
import org.apache.arrow.vector.ipc.message.ArrowFieldNode;
import org.apache.arrow.vector.ipc.message.ArrowRecordBatch;
import org.apache.arrow.vector.ipc.message.MessageSerializer;
import org.junit.After;
import org.junit.Before;
import org.junit.Test;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.channels.Channels;
import java.util.Collections;
import java.util.List;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertThrows;

public class ArrowRecordBatchReaderTest
{
    private BufferAllocator allocator;

    @Before
    public void setUp()
    {
        allocator = new RootAllocator();
    }

    @After
    public void tearDown()
    {
        allocator.close();
    }

    @Test
    public void readsBatchWithoutColumnsAndEmptyBody() throws IOException
    {
        byte[] bytes = message(3, Collections.emptyList(), Collections.emptyList());

        try (ArrowRecordBatch batch = ArrowRecordBatchReader.deserialize(bytes, allocator)) {
            assertEquals(3, batch.getLength());
            assertEquals(0, batch.getNodes().size());
            assertEquals(0, batch.getBuffers().size());
        }
    }

    @Test
    public void readsBatchWithEmptyBuffersAndEmptyBody() throws IOException
    {
        byte[] bytes = message(0, List.of(new ArrowFieldNode(0, 0)), List.of(allocator.getEmpty(), allocator.getEmpty()));

        try (ArrowRecordBatch batch = ArrowRecordBatchReader.deserialize(bytes, allocator)) {
            assertEquals(0, batch.getLength());
            assertEquals(2, batch.getBuffers().size());
        }
    }

    @Test
    public void rejectsBufferOutsideEmptyBody() throws IOException
    {
        try (ArrowBuf validity = allocator.buffer(8); ArrowBuf values = allocator.buffer(16)) {
            validity.writerIndex(8);
            values.writerIndex(16);
            byte[] bytes = message(3, List.of(new ArrowFieldNode(3, 0)), List.of(validity, values));

            IndexOutOfBoundsException error = assertThrows(IndexOutOfBoundsException.class,
                    () -> ArrowRecordBatchReader.deserialize(bytes, allocator));
            assertEquals("index (8) must not be greater than size (0)", error.getMessage());
        }
    }

    private static byte[] message(int length, List<ArrowFieldNode> nodes, List<ArrowBuf> buffers) throws IOException
    {
        FlatBufferBuilder builder = new FlatBufferBuilder();
        int header;
        try (ArrowRecordBatch batch = new ArrowRecordBatch(length, nodes, buffers)) {
            header = batch.writeTo(builder);
        }
        ByteBuffer metadata = MessageSerializer.serializeMessage(builder, MessageHeader.RecordBatch, header, 0);
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        WriteChannel channel = new WriteChannel(Channels.newChannel(out));
        MessageSerializer.writeMessageBuffer(channel, metadata.remaining(), metadata, AthenaFederationIpcOption.DEFAULT);
        return out.toByteArray();
    }
}
