pub(crate) mod arrow_ipc;
pub mod base;
pub mod block_deserializer;
pub mod block_serializer;
pub mod federated_identity_serde;
pub mod java_hash;
pub(crate) mod json;
pub(crate) mod object_mapper;
pub mod ping_request_serde;
pub mod ping_response_serde;
pub mod schema_deserializer;
pub mod schema_serializer;
pub mod v1;
pub mod v2;
pub mod v3;
pub mod v4;
pub mod v5;
pub mod v6;

use std::fmt;

use crate::data::{BlockAllocator, BlockAllocatorRegistry};

pub use arrow_ipc::{
    block_batch_bytes, record_batch_from_bytes, record_batch_to_bytes, schema_from_bytes,
    schema_to_bytes, IpcFormat,
};
pub use json::{
    decode_base64, encode_base64, parse_as_int, parse_as_long, token_name, JsonGenerator,
    JsonParser, JsonToken,
};
pub use object_mapper::ObjectMapper;
pub use v2::lambda_function_exception_serde::LambdaException;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SerdeVersion {
    V1 = 1,
    V2 = 2,
    V3 = 3,
    V4 = 4,
    V5 = 5,
    V6 = 6,
}

impl SerdeVersion {
    pub const CURRENT: SerdeVersion = SerdeVersion::V6;

    pub fn as_i32(self) -> i32 {
        self as i32
    }

    pub fn lower(self) -> Option<SerdeVersion> {
        match self {
            SerdeVersion::V1 => None,
            SerdeVersion::V2 => Some(SerdeVersion::V1),
            SerdeVersion::V3 => Some(SerdeVersion::V2),
            SerdeVersion::V4 => Some(SerdeVersion::V3),
            SerdeVersion::V5 => Some(SerdeVersion::V4),
            SerdeVersion::V6 => Some(SerdeVersion::V5),
        }
    }

    pub fn is_strict(self) -> bool {
        self > SerdeVersion::V1
    }

    pub fn ipc_format(self) -> IpcFormat {
        if self <= SerdeVersion::V2 {
            IpcFormat::AthenaFederationLegacy
        } else {
            IpcFormat::ArrowDefault
        }
    }
}

impl fmt::Display for SerdeVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_i32())
    }
}

pub const SERDE_VERSION: i32 = SerdeVersion::CURRENT as i32;

#[derive(Clone, Copy)]
pub enum AllocatorSource<'a> {
    Allocator(&'a dyn BlockAllocator),
    Registry(&'a dyn BlockAllocatorRegistry),
}

#[derive(Clone, Copy)]
pub struct SerDeContext<'a> {
    pub version: SerdeVersion,
    pub allocators: AllocatorSource<'a>,
}

impl<'a> SerDeContext<'a> {
    pub fn new(version: SerdeVersion, allocator: &'a dyn BlockAllocator) -> SerDeContext<'a> {
        SerDeContext {
            version,
            allocators: AllocatorSource::Allocator(allocator),
        }
    }

    pub fn with_registry(
        version: SerdeVersion,
        registry: &'a dyn BlockAllocatorRegistry,
    ) -> SerDeContext<'a> {
        SerDeContext {
            version,
            allocators: AllocatorSource::Registry(registry),
        }
    }
}
