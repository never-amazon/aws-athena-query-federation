use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;

use arrow::buffer::Buffer;
use arrow::datatypes::{Schema, SchemaRef};
use arrow::ipc::writer::{DictionaryTracker, IpcDataGenerator, IpcWriteContext, IpcWriteOptions};
use arrow::ipc::{root_as_message, MessageHeader, MetadataVersion};
use arrow::record_batch::RecordBatch;

use super::block_allocator::BlockAllocator;
use crate::exceptions::{FederationError, Result};

const CONTINUATION_MARKER: [u8; 4] = [0xff; 4];

fn ipc_write_options() -> Result<IpcWriteOptions> {
    Ok(IpcWriteOptions::try_new(8, true, MetadataVersion::V4)?)
}

#[derive(Debug, Clone)]
pub struct ArrowRecordBatch {
    length: usize,
    metadata: Vec<u8>,
    body: Buffer,
    version: MetadataVersion,
}

impl ArrowRecordBatch {
    pub fn from_record_batch(batch: &RecordBatch) -> Result<ArrowRecordBatch> {
        let options = ipc_write_options()?;
        let generator = IpcDataGenerator::default();
        let mut tracker = DictionaryTracker::new(false);
        let mut ctx = IpcWriteContext::default();
        let (_, encoded) = generator.encode(batch, &mut tracker, &options, &mut ctx)?;
        Ok(ArrowRecordBatch {
            length: batch.num_rows(),
            metadata: encoded.ipc_message,
            body: Buffer::from_vec(encoded.arrow_data),
            version: MetadataVersion::V4,
        })
    }

    fn from_message(metadata: Vec<u8>, body: Vec<u8>) -> Result<ArrowRecordBatch> {
        let message = root_as_message(&metadata)
            .map_err(|e| FederationError::Io(format!("Unable to read IPC message: {e}")))?;
        if message.header_type() != MessageHeader::RecordBatch {
            return Err(FederationError::Io(format!(
                "Expected RecordBatch but header was {:?}",
                message.header_type()
            )));
        }
        let header = message.header_as_record_batch();
        if body.is_empty() {
            if let Some(message) = header
                .and_then(|b| b.buffers())
                .into_iter()
                .flatten()
                .find_map(|buffer| empty_body_slice_error(buffer.offset(), buffer.length()))
            {
                return Err(FederationError::IndexOutOfBounds(message));
            }
        }
        let length = header.map_or(0, |b| usize::try_from(b.length()).unwrap_or_default());
        let version = message.version();
        Ok(ArrowRecordBatch {
            length,
            metadata,
            body: Buffer::from_vec(body),
            version,
        })
    }

    pub fn length(&self) -> usize {
        self.length
    }

    pub fn compute_body_length(&self) -> usize {
        self.body.len()
    }

    pub fn decode(&self, schema: SchemaRef) -> Result<RecordBatch> {
        let message = root_as_message(&self.metadata)
            .map_err(|e| FederationError::Io(format!("Unable to read IPC message: {e}")))?;
        let fb_batch = message
            .header_as_record_batch()
            .ok_or_else(|| FederationError::Io("Message is not a record batch".into()))?;
        let dictionaries = HashMap::new();
        let batch = arrow::ipc::reader::read_record_batch(
            &self.body,
            fb_batch,
            schema,
            &dictionaries,
            None,
            &self.version,
        )?;
        Ok(batch)
    }

    pub fn serialize(&self, out: &mut dyn Write) -> Result<()> {
        let options = ipc_write_options()?;
        let encoded = arrow::ipc::writer::EncodedData {
            ipc_message: self.metadata.clone(),
            arrow_data: self.body.as_slice().to_vec(),
        };
        arrow::ipc::writer::write_message(out, encoded, &options)?;
        Ok(())
    }
}

fn empty_body_slice_error(offset: i64, length: i64) -> Option<String> {
    let position_error = |index: i64| match index {
        0 => None,
        i if i < 0 => Some(format!("index ({i}) must not be negative")),
        i => Some(format!("index ({i}) must not be greater than size (0)")),
    };
    position_error(offset).or_else(|| position_error(length))
}

fn read_message(input: &mut dyn Read) -> Result<Option<(Vec<u8>, usize)>> {
    let mut prefix = [0u8; 4];
    if let Err(e) = input.read_exact(&mut prefix) {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            return Ok(None);
        }
        return Err(e.into());
    }
    if prefix == CONTINUATION_MARKER {
        input.read_exact(&mut prefix)?;
    }
    let len = i32::from_le_bytes(prefix);
    if len <= 0 {
        return Ok(None);
    }
    let mut meta = vec![0u8; len.unsigned_abs() as usize];
    input.read_exact(&mut meta)?;
    let message = root_as_message(&meta)
        .map_err(|e| FederationError::Io(format!("Unable to read IPC message: {e}")))?;
    let body_len = usize::try_from(message.bodyLength()).map_err(|_| {
        FederationError::Io(format!(
            "Invalid IPC message body length {}",
            message.bodyLength()
        ))
    })?;
    Ok(Some((meta, body_len)))
}

pub fn deserialize_record_batch_message(input: &mut dyn Read) -> Result<ArrowRecordBatch> {
    let (meta, body_len) = read_message(input)?.ok_or_else(|| {
        FederationError::Io("Unexpected end of input reading record batch".into())
    })?;
    let mut body = vec![0u8; body_len];
    input.read_exact(&mut body)?;
    ArrowRecordBatch::from_message(meta, body)
}

pub fn serialize_record_batch(batch: &RecordBatch, out: &mut dyn Write) -> Result<()> {
    ArrowRecordBatch::from_record_batch(batch)?.serialize(out)
}

pub struct RecordBatchSerDe<'a> {
    allocator: &'a dyn BlockAllocator,
}

impl<'a> RecordBatchSerDe<'a> {
    pub fn new(allocator: &'a dyn BlockAllocator) -> RecordBatchSerDe<'a> {
        RecordBatchSerDe { allocator }
    }

    pub fn serialize(&self, batch: &ArrowRecordBatch, out: &mut dyn Write) -> Result<()> {
        batch.serialize(out)
    }

    pub fn serialize_record_batch(&self, batch: &RecordBatch, out: &mut dyn Write) -> Result<()> {
        serialize_record_batch(batch, out)
    }

    pub fn deserialize(&self, input: &[u8]) -> Result<ArrowRecordBatch> {
        let mut cursor = std::io::Cursor::new(input);
        self.allocator
            .register_batch(&mut || deserialize_record_batch_message(&mut cursor))
    }
}

pub fn serialize_schema(schema: &Schema, out: &mut dyn Write) -> Result<()> {
    let options = ipc_write_options()?;
    let generator = IpcDataGenerator::default();
    let mut tracker = DictionaryTracker::new(false);
    let encoded = generator.schema_to_bytes_with_dictionary_tracker(schema, &mut tracker, &options);
    arrow::ipc::writer::write_message(out, encoded, &options)?;
    Ok(())
}

pub fn deserialize_schema(input: &mut dyn Read) -> Result<Schema> {
    let (meta, body_len) = read_message(input)?
        .ok_or_else(|| FederationError::Io("Unexpected end of input reading schema".into()))?;
    if body_len > 0 {
        let mut skip = vec![0u8; body_len];
        input.read_exact(&mut skip)?;
    }
    let message = root_as_message(&meta)
        .map_err(|e| FederationError::Io(format!("Unable to read IPC message: {e}")))?;
    let fb = message.header_as_schema().ok_or_else(|| {
        FederationError::Io(format!(
            "Expected schema but header was {:?}",
            message.header_type()
        ))
    })?;
    Ok(arrow::ipc::convert::try_fb_to_schema(fb)?)
}

pub fn schema_ref(schema: Schema) -> SchemaRef {
    Arc::new(schema)
}
