use base64::alphabet;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::engine::DecodePaddingMode;
use base64::Engine;
use prost::Message;

use crate::error::{Result, SubstraitError};
use crate::proto::rel::RelType;
use crate::proto::{FetchRel, FilterRel, Plan, ProjectRel, ReadRel, Rel, SortRel};

pub const PLAN_PARSER_RECURSION_LIMIT: usize = 2000;

const TOO_DEEP_MESSAGE: &str = "Protocol message had too many levels of nesting.  May be malicious.  Use CodedInputStream.setRecursionLimit() to increase the depth limit.";

const JAVA_BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

pub struct SubstraitRelUtils;

impl SubstraitRelUtils {
    fn traverse_and_extract_rel<T>(
        rel: Option<&Rel>,
        extract: fn(&RelType) -> Option<&T>,
    ) -> Option<&T> {
        let mut current = rel;
        while let Some(r) = current {
            let rel_type = r.rel_type.as_ref()?;
            if let Some(found) = extract(rel_type) {
                return Some(found);
            }
            current = match rel_type {
                RelType::Filter(f) => f.input.as_deref(),
                RelType::Fetch(f) => f.input.as_deref(),
                RelType::Sort(s) => s.input.as_deref(),
                RelType::Aggregate(a) => a.input.as_deref(),
                RelType::Project(p) => p.input.as_deref(),
                _ => None,
            };
        }
        None
    }

    pub fn get_filter_rel(rel: Option<&Rel>) -> Option<&FilterRel> {
        Self::traverse_and_extract_rel(rel, |t| match t {
            RelType::Filter(f) => Some(f.as_ref()),
            _ => None,
        })
    }

    pub fn get_fetch_rel(rel: Option<&Rel>) -> Option<&FetchRel> {
        Self::traverse_and_extract_rel(rel, |t| match t {
            RelType::Fetch(f) => Some(f.as_ref()),
            _ => None,
        })
    }

    pub fn get_sort_rel(rel: Option<&Rel>) -> Option<&SortRel> {
        Self::traverse_and_extract_rel(rel, |t| match t {
            RelType::Sort(s) => Some(s.as_ref()),
            _ => None,
        })
    }

    pub fn get_read_rel(rel: Option<&Rel>) -> Option<&ReadRel> {
        Self::traverse_and_extract_rel(rel, |t| match t {
            RelType::Read(r) => Some(r.as_ref()),
            _ => None,
        })
    }

    pub fn get_project_rel(rel: Option<&Rel>) -> Option<&ProjectRel> {
        Self::traverse_and_extract_rel(rel, |t| match t {
            RelType::Project(p) => Some(p.as_ref()),
            _ => None,
        })
    }

    pub fn deserialize_substrait_plan(plan_string: &str) -> Result<Plan> {
        let wrap =
            |cause: SubstraitError| SubstraitError::wrap("Failed to parse Substrait plan", cause);
        let bytes = JAVA_BASE64
            .decode(plan_string)
            .map_err(|e| wrap(SubstraitError::illegal_argument(e.to_string())))?;
        if let Err(DepthCheck::TooDeep) = check_message_depth(&bytes, 0) {
            return Err(wrap(SubstraitError::runtime(TOO_DEEP_MESSAGE)));
        }
        Plan::decode(bytes.as_slice()).map_err(|e| wrap(SubstraitError::runtime(e.to_string())))
    }
}

#[derive(Debug, PartialEq, Eq)]
enum DepthCheck {
    TooDeep,
    Malformed,
}

fn read_varint(buf: &[u8], pos: &mut usize) -> std::result::Result<u64, DepthCheck> {
    let mut value: u64 = 0;
    let mut shift = 0u32;
    loop {
        let byte = *buf.get(*pos).ok_or(DepthCheck::Malformed)?;
        *pos += 1;
        if shift >= 64 {
            return Err(DepthCheck::Malformed);
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
    }
}

fn check_message_depth(buf: &[u8], depth: usize) -> std::result::Result<(), DepthCheck> {
    let mut pos = 0usize;
    check_fields(buf, &mut pos, depth, None)
}

fn check_fields(
    buf: &[u8],
    pos: &mut usize,
    depth: usize,
    end_group: Option<u64>,
) -> std::result::Result<(), DepthCheck> {
    if depth > PLAN_PARSER_RECURSION_LIMIT {
        return Err(DepthCheck::TooDeep);
    }
    while *pos < buf.len() {
        let key = read_varint(buf, pos)?;
        let field = key >> 3;
        if field == 0 {
            return Err(DepthCheck::Malformed);
        }
        match key & 7 {
            0 => {
                read_varint(buf, pos)?;
            }
            1 => {
                *pos = pos
                    .checked_add(8)
                    .filter(|p| *p <= buf.len())
                    .ok_or(DepthCheck::Malformed)?;
            }
            2 => {
                let len =
                    usize::try_from(read_varint(buf, pos)?).map_err(|_| DepthCheck::Malformed)?;
                let end = pos
                    .checked_add(len)
                    .filter(|p| *p <= buf.len())
                    .ok_or(DepthCheck::Malformed)?;
                let sub = &buf[*pos..end];
                if let Err(DepthCheck::TooDeep) = check_message_depth(sub, depth + 1) {
                    return Err(DepthCheck::TooDeep);
                }
                *pos = end;
            }
            3 => {
                check_fields(buf, pos, depth + 1, Some(field))?;
            }
            4 => {
                return if end_group == Some(field) {
                    Ok(())
                } else {
                    Err(DepthCheck::Malformed)
                };
            }
            5 => {
                *pos = pos
                    .checked_add(4)
                    .filter(|p| *p <= buf.len())
                    .ok_or(DepthCheck::Malformed)?;
            }
            _ => return Err(DepthCheck::Malformed),
        }
    }
    if end_group.is_some() {
        return Err(DepthCheck::Malformed);
    }
    Ok(())
}
