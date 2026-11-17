pub mod arrow_ipc;
pub mod base;
pub mod block_deserializer;
pub mod block_serializer;
pub mod federated_identity_serde;
pub mod java_hash;
pub mod json;
pub mod object_mapper;
pub mod object_mapper_factory;
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
pub mod versioned_object_mapper_factory;

use crate::data::{BlockAllocator, BlockAllocatorRegistry};

pub use arrow_ipc::IpcFormat;
pub use json::{JsonGenerator, JsonParser, JsonToken};
pub use object_mapper::ObjectMapper;
pub use v2::lambda_function_exception_serde::LambdaException;
pub use versioned_object_mapper_factory::VersionedObjectMapperFactory;

pub const SERDE_VERSION: i32 = 6;
pub const SERDE_VERSION_ONE: i32 = 1;
pub const SERDE_VERSION_TWO: i32 = 2;
pub const SERDE_VERSION_THREE: i32 = 3;
pub const SERDE_VERSION_FOUR: i32 = 4;
pub const SERDE_VERSION_FIVE: i32 = 5;
pub const SERDE_VERSION_SIX: i32 = 6;

#[derive(Clone, Copy)]
pub enum AllocatorSource<'a> {
    Allocator(&'a dyn BlockAllocator),
    Registry(&'a dyn BlockAllocatorRegistry),
}

#[derive(Clone, Copy)]
pub struct SerDeContext<'a> {
    pub version: i32,
    pub allocators: AllocatorSource<'a>,
}

impl<'a> SerDeContext<'a> {
    pub fn new(version: i32, allocator: &'a dyn BlockAllocator) -> SerDeContext<'a> {
        SerDeContext {
            version,
            allocators: AllocatorSource::Allocator(allocator),
        }
    }

    pub fn with_registry(version: i32, registry: &'a dyn BlockAllocatorRegistry) -> SerDeContext<'a> {
        SerDeContext {
            version,
            allocators: AllocatorSource::Registry(registry),
        }
    }

    pub fn ipc_format(&self) -> IpcFormat {
        ipc_format_for(self.version)
    }
}

pub fn ipc_format_for(version: i32) -> IpcFormat {
    if version <= 2 {
        IpcFormat::AthenaFederationLegacy
    } else {
        IpcFormat::ArrowDefault
    }
}
