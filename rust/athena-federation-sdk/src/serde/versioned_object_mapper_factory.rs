use std::sync::Arc;

use crate::data::BlockAllocator;
use crate::exceptions::{FederationError, Result};
use crate::request::FederationRequest;
use crate::serde::{object_mapper_factory, v2, v3, v4, v5, v6, ObjectMapper, SERDE_VERSION};

pub struct VersionedObjectMapperFactory;

impl VersionedObjectMapperFactory {
    pub fn create(allocator: Arc<dyn BlockAllocator>) -> ObjectMapper {
        match VersionedObjectMapperFactory::create_with_version(allocator.clone(), SERDE_VERSION) {
            Ok(mapper) => mapper,
            Err(_) => v6::object_mapper_factory_v6::create(allocator),
        }
    }

    pub fn create_with_version(
        allocator: Arc<dyn BlockAllocator>,
        version: i32,
    ) -> Result<ObjectMapper> {
        tracing::debug!("Athena SDK SerDe Version: {}", version);
        match version {
            1 => Ok(object_mapper_factory::create(allocator)),
            2 => Ok(v2::object_mapper_factory_v2::create(allocator)),
            3 => Ok(v3::object_mapper_factory_v3::create(allocator)),
            4 => Ok(v4::object_mapper_factory_v4::create(allocator)),
            5 => Ok(v5::object_mapper_factory_v5::create(allocator)),
            6 => Ok(v6::object_mapper_factory_v6::create(allocator)),
            other => Err(FederationError::invalid_input(format!(
                "No serde version {other}"
            ))),
        }
    }

    pub fn read_request_with_version_fallback(
        allocator: &Arc<dyn BlockAllocator>,
        input: &[u8],
    ) -> Result<(ObjectMapper, FederationRequest)> {
        let mut resolved_ser_de_version = SERDE_VERSION;
        while resolved_ser_de_version >= 1 {
            let mapper = VersionedObjectMapperFactory::create_with_version(
                allocator.clone(),
                resolved_ser_de_version,
            )?;
            match mapper.read_request(input) {
                Ok(request) => {
                    tracing::info!(
                        "Parsing request with resolvedSerDeVersion: '{}', connector SerDeVersion: '{}'",
                        resolved_ser_de_version,
                        SERDE_VERSION
                    );
                    return Ok((mapper, request));
                }
                Err(FederationError::IllegalState(_)) => {
                    resolved_ser_de_version -= 1;
                    tracing::debug!(
                        "Client's SerDe mis-matched with connector version:, attempt with lower version: '{}'",
                        resolved_ser_de_version
                    );
                }
                Err(e) => return Err(e),
            }
        }
        Err(FederationError::invalid_input(format!(
            "FederationRequest/ObjectMapper is null with SerDeVersion: '{resolved_ser_de_version}'"
        )))
    }
}
