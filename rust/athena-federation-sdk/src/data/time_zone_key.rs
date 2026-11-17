use std::collections::BTreeMap;
use std::fmt;
use std::sync::OnceLock;

use crate::exceptions::{FederationError, Result};

const ZONE_INDEX: &str = include_str!("zone-index.properties");
const OFFSET_TIME_ZONE_MIN: i16 = -14 * 60;
const OFFSET_TIME_ZONE_MAX: i16 = 14 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TimeZoneKey {
    id: String,
    key: i16,
}

struct ZoneIndex {
    zone_id_to_key: BTreeMap<String, TimeZoneKey>,
    time_zone_keys: Vec<Option<TimeZoneKey>>,
    offset_time_zone_keys: Vec<Option<TimeZoneKey>>,
    max_time_zone_key: i16,
}

fn parse_properties(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_start();
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        let sep = line
            .char_indices()
            .find(|(_, c)| c.is_whitespace() || *c == '=' || *c == ':')
            .map_or(line.len(), |(i, _)| i);
        let key = &line[..sep];
        let mut rest = line[sep..].trim_start();
        if rest.starts_with('=') || rest.starts_with(':') {
            rest = rest[1..].trim_start();
        }
        out.push((key.to_string(), rest.to_string()));
    }
    out
}

fn zone_id_for_offset(offset: i64) -> String {
    format!(
        "{}{:02}:{:02}",
        if offset < 0 { "-" } else { "+" },
        (offset / 60).abs(),
        (offset % 60).abs()
    )
}

fn index() -> &'static ZoneIndex {
    static INDEX: OnceLock<ZoneIndex> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut zone_id_to_key: BTreeMap<String, TimeZoneKey> = BTreeMap::new();
        zone_id_to_key.insert("utc".to_string(), TimeZoneKey::utc_key());
        let mut max_key: i16 = 0;
        for (k, v) in parse_properties(ZONE_INDEX) {
            let key: i16 = match k.trim().parse() {
                Ok(x) => x,
                Err(_) => continue,
            };
            let id = v.trim().to_string();
            max_key = max_key.max(key);
            zone_id_to_key.insert(id.to_lowercase(), TimeZoneKey { id, key });
        }
        let mut time_zone_keys = vec![None; usize::from(max_key.unsigned_abs()) + 1];
        for tzk in zone_id_to_key.values() {
            if let Ok(slot) = usize::try_from(tzk.key) {
                time_zone_keys[slot] = Some(tzk.clone());
            }
        }
        let mut offset_time_zone_keys =
            vec![
                None;
                usize::from((OFFSET_TIME_ZONE_MAX - OFFSET_TIME_ZONE_MIN).unsigned_abs()) + 1
            ];
        for offset in OFFSET_TIME_ZONE_MIN..=OFFSET_TIME_ZONE_MAX {
            if offset == 0 {
                continue;
            }
            let zone_id = zone_id_for_offset(i64::from(offset));
            offset_time_zone_keys[usize::from((offset - OFFSET_TIME_ZONE_MIN).unsigned_abs())] =
                zone_id_to_key.get(&zone_id).cloned();
        }
        ZoneIndex {
            zone_id_to_key,
            time_zone_keys,
            offset_time_zone_keys,
            max_time_zone_key: max_key,
        }
    })
}

fn is_utc_equivalent_name(zone_id: &str) -> bool {
    matches!(
        zone_id,
        "utc" | "z" | "ut" | "uct" | "gmt" | "gmt0" | "greenwich" | "universal" | "zulu"
    )
}

fn normalize_zone_id(original: &str) -> String {
    let mut zone_id = original.to_lowercase();
    let starts_with_etc = zone_id.starts_with("etc/");
    if starts_with_etc {
        zone_id = zone_id[4..].to_string();
    }
    if is_utc_equivalent_name(&zone_id) {
        return "utc".to_string();
    }
    let mut starts_with_etc_gmt = false;
    if zone_id.len() > 3 && (zone_id.starts_with("utc") || zone_id.starts_with("gmt")) {
        if starts_with_etc && zone_id.starts_with("gmt") {
            starts_with_etc_gmt = true;
        }
        zone_id = zone_id[3..].to_string();
    } else if zone_id.len() > 2 && zone_id.starts_with("ut") {
        zone_id = zone_id[2..].to_string();
    }
    if zone_id == "+00:00" || zone_id == "-00:00" {
        return "utc".to_string();
    }
    let chars: Vec<char> = zone_id.chars().collect();
    let length = chars.len();
    if length == 6 && chars[3] == ':' {
        return zone_id;
    }
    if length != 2 && length != 3 {
        return original.to_string();
    }
    let mut sign = chars[0];
    if sign != '+' && sign != '-' {
        return original.to_string();
    }
    if starts_with_etc_gmt {
        sign = if sign == '-' { '+' } else { '-' };
    }
    let (tens, ones) = if length == 2 {
        ('0', chars[1])
    } else {
        (chars[1], chars[2])
    };
    if !tens.is_ascii_digit() || !ones.is_ascii_digit() {
        return original.to_string();
    }
    if tens == '0' && ones == '0' {
        return "utc".to_string();
    }
    format!("{sign}{tens}{ones}:00")
}

impl TimeZoneKey {
    pub fn utc_key() -> TimeZoneKey {
        TimeZoneKey {
            id: "UTC".to_string(),
            key: 0,
        }
    }

    pub fn new(id: impl Into<String>, key: i16) -> Result<TimeZoneKey> {
        if key < 0 {
            return Err(FederationError::invalid_input("key is negative"));
        }
        Ok(TimeZoneKey { id: id.into(), key })
    }

    pub fn max_time_zone_key() -> i16 {
        index().max_time_zone_key
    }

    pub fn get_time_zone_keys() -> Vec<TimeZoneKey> {
        index().zone_id_to_key.values().cloned().collect()
    }

    pub fn get_time_zone_key(key: i16) -> Result<TimeZoneKey> {
        let idx = index();
        if let Some(Some(k)) = usize::try_from(key)
            .ok()
            .and_then(|slot| idx.time_zone_keys.get(slot))
        {
            return Ok(k.clone());
        }
        Err(FederationError::invalid_input(format!(
            "Invalid time zone key {key}"
        )))
    }

    pub fn get_time_zone_key_for_id(zone_id: &str) -> Result<TimeZoneKey> {
        if zone_id.is_empty() {
            return Err(FederationError::invalid_input("Zone id is an empty string"));
        }
        let idx = index();
        if let Some(k) = idx.zone_id_to_key.get(&zone_id.to_lowercase()) {
            return Ok(k.clone());
        }
        if let Some(k) = idx.zone_id_to_key.get(&normalize_zone_id(zone_id)) {
            return Ok(k.clone());
        }
        Err(FederationError::invalid_input(format!(
            "Time zone not supported: {zone_id}"
        )))
    }

    pub fn get_time_zone_key_for_offset(offset_minutes: i64) -> Result<TimeZoneKey> {
        if offset_minutes == 0 {
            return Ok(TimeZoneKey::utc_key());
        }
        if !(i64::from(OFFSET_TIME_ZONE_MIN)..=i64::from(OFFSET_TIME_ZONE_MAX))
            .contains(&offset_minutes)
        {
            return Err(FederationError::invalid_input(format!(
                "Invalid offset minutes {offset_minutes}"
            )));
        }
        let slot = usize::try_from(offset_minutes - i64::from(OFFSET_TIME_ZONE_MIN)).ok();
        match slot.and_then(|i| index().offset_time_zone_keys.get(i)) {
            Some(Some(k)) => Ok(k.clone()),
            Some(None) | None => Err(FederationError::invalid_input(format!(
                "Time zone not supported: {}",
                zone_id_for_offset(offset_minutes)
            ))),
        }
    }

    pub fn is_utc_zone_id(zone_id: &str) -> bool {
        normalize_zone_id(zone_id) == "utc"
    }

    pub fn get_id(&self) -> &str {
        &self.id
    }

    pub fn get_key(&self) -> i16 {
        self.key
    }
}

impl fmt::Display for TimeZoneKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.id)
    }
}
