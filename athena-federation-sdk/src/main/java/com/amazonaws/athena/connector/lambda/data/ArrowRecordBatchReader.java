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

import org.apache.arrow.flatbuf.Message;
import org.apache.arrow.flatbuf.MessageHeader;
import org.apache.arrow.memory.BufferAllocator;
import org.apache.arrow.vector.ipc.ReadChannel;
import org.apache.arrow.vector.ipc.message.ArrowRecordBatch;
import org.apache.arrow.vector.ipc.message.MessageMetadataResult;
import org.apache.arrow.vector.ipc.message.MessageSerializer;
import org.apache.arrow.vector.types.MetadataVersion;

import java.io.ByteArrayInputStream;
import java.io.IOException;
import java.nio.channels.Channels;

public final class ArrowRecordBatchReader
{
    private ArrowRecordBatchReader()
    {
    }

    public static ArrowRecordBatch deserialize(byte[] in, BufferAllocator allocator)
            throws IOException
    {
        MessageMetadataResult metadata = MessageSerializer.readMessage(channel(in));
        if (metadata != null && metadata.getMessageBodyLength() == 0 && isRecordBatch(metadata.getMessage())) {
            return MessageSerializer.deserializeRecordBatch(metadata.getMessage(), allocator.getEmpty());
        }
        return (ArrowRecordBatch) MessageSerializer.deserializeMessageBatch(channel(in), allocator);
    }

    private static boolean isRecordBatch(Message message)
    {
        short version = message.version();
        boolean supported = version == MetadataVersion.V4.toFlatbufID() || version == MetadataVersion.V5.toFlatbufID();
        return supported && message.headerType() == MessageHeader.RecordBatch;
    }

    private static ReadChannel channel(byte[] in)
    {
        return new ReadChannel(Channels.newChannel(new ByteArrayInputStream(in)));
    }
}
