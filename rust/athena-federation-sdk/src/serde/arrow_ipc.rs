use arrow::datatypes::Schema;
use arrow::ipc::writer::{
    write_message, DictionaryTracker, IpcDataGenerator, IpcWriteContext, IpcWriteOptions,
};
use arrow::ipc::MetadataVersion;
use arrow::record_batch::RecordBatch;

use crate::data::record_batch_serde::{deserialize_schema, RecordBatchSerDe};
use crate::data::{ArrowRecordBatch, Block, BlockAllocator};
use crate::exceptions::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IpcFormat {
    AthenaFederationLegacy,
    ArrowDefault,
}

impl IpcFormat {
    pub fn write_options(&self) -> Result<IpcWriteOptions> {
        Ok(match self {
            IpcFormat::AthenaFederationLegacy => {
                IpcWriteOptions::try_new(8, true, MetadataVersion::V4)?
            }
            IpcFormat::ArrowDefault => IpcWriteOptions::try_new(8, false, MetadataVersion::V5)?,
        })
    }
}

pub fn schema_to_bytes(schema: &Schema, format: IpcFormat) -> Result<Vec<u8>> {
    let options = format.write_options()?;
    let generator = IpcDataGenerator::default();
    let mut tracker = DictionaryTracker::new(false);
    let encoded = generator.schema_to_bytes_with_dictionary_tracker(schema, &mut tracker, &options);
    let mut out = Vec::new();
    write_message(&mut out, encoded, &options)?;
    Ok(out)
}

pub fn record_batch_to_bytes(batch: &RecordBatch, format: IpcFormat) -> Result<Vec<u8>> {
    let options = format.write_options()?;
    let generator = IpcDataGenerator::default();
    let mut tracker = DictionaryTracker::new(false);
    let mut ctx = IpcWriteContext::default();
    let (_, encoded) = generator.encode(batch, &mut tracker, &options, &mut ctx)?;
    let mut out = Vec::new();
    write_message(&mut out, encoded, &options)?;
    Ok(out)
}

pub fn schema_from_bytes(bytes: &[u8]) -> Result<Schema> {
    let mut cursor = std::io::Cursor::new(bytes);
    deserialize_schema(&mut cursor)
}

pub fn record_batch_from_bytes(
    allocator: &dyn BlockAllocator,
    bytes: &[u8],
) -> Result<ArrowRecordBatch> {
    RecordBatchSerDe::new(allocator).deserialize(bytes)
}

pub fn block_batch_bytes(block: &Block, format: IpcFormat) -> Result<Vec<u8>> {
    let batch = block.get_record_batch()?;
    record_batch_to_bytes(&batch, format)
}
