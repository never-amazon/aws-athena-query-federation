use athena_federation_sdk::data::TimeZoneKey;
use athena_federation_sdk::exceptions::FederationError;

fn utc_key() -> TimeZoneKey {
    TimeZoneKey::utc_key()
}

fn plus_7_key() -> TimeZoneKey {
    TimeZoneKey::get_time_zone_key_for_offset(7 * 60).unwrap()
}

fn minus_7_key() -> TimeZoneKey {
    TimeZoneKey::get_time_zone_key_for_offset(-7 * 60).unwrap()
}

fn key(id: &str) -> TimeZoneKey {
    TimeZoneKey::get_time_zone_key_for_id(id).unwrap_or_else(|e| panic!("{} failed: {}", id, e))
}

fn assert_time_zone_not_supported(zone_id: &str) {
    match TimeZoneKey::get_time_zone_key_for_id(zone_id) {
        Err(FederationError::AthenaConnector(_)) => {}
        other => panic!("expect IllegalArgumentException for {}, got {:?}", zone_id, other),
    }
}

#[test]
fn test_utc() {
    assert_eq!(utc_key().get_key(), 0);
    assert_eq!(utc_key().get_id(), "UTC");
    assert_eq!(TimeZoneKey::get_time_zone_key(0).unwrap(), utc_key());
    for id in [
        "UTC", "Z", "Zulu", "zulu", "ZULU", "UT", "UCT", "Universal", "GMT", "GMT0", "GMT+0", "GMT-0", "GMT+00:00",
        "GMT-00:00", "+00:00", "-00:00", "etc/utc", "etc/gmt", "etc/gmt+0", "etc/gmt+00:00", "etc/gmt-00:00", "etc/ut",
        "etc/UT", "etc/UCT", "etc/Universal",
    ] {
        assert_eq!(key(id), utc_key(), "{}", id);
    }
}

#[test]
fn test_hour_offset_zone() {
    for prefix in ["GMT", "UT", "UTC", "Etc/GMT", "Etc/UT", "Etc/UTC"] {
        if prefix == "GMT" || prefix == "Etc/GMT" {
            assert_eq!(key(&format!("{}0", prefix)), utc_key());
        } else {
            assert_time_zone_not_supported(&format!("{}0", prefix));
        }
        for _ in 0..2 {
            assert_eq!(key(&format!("{}+0", prefix)), utc_key());
            assert_eq!(key(&format!("{}-0", prefix)), utc_key());
        }
        assert_time_zone_not_supported(&format!("{}7", prefix));
        let (plus, minus) = if prefix == "Etc/GMT" {
            (minus_7_key(), plus_7_key())
        } else {
            (plus_7_key(), minus_7_key())
        };
        for _ in 0..2 {
            assert_eq!(key(&format!("{}+7", prefix)), plus, "{}+7", prefix);
            assert_eq!(key(&format!("{}-7", prefix)), minus, "{}-7", prefix);
        }
    }
}

#[test]
fn test_zone_key_lookup() {
    for tzk in TimeZoneKey::get_time_zone_keys() {
        assert_eq!(TimeZoneKey::get_time_zone_key(tzk.get_key()).unwrap(), tzk);
        assert_eq!(key(tzk.get_id()), tzk);
        assert_eq!(key(&tzk.get_id().to_uppercase()), tzk);
        assert_eq!(key(&tzk.get_id().to_lowercase()), tzk);
    }
}

#[test]
fn test_max_time_zone_key() {
    let max = TimeZoneKey::max_time_zone_key();
    let mut found_max = false;
    for tzk in TimeZoneKey::get_time_zone_keys() {
        assert!(tzk.get_key() <= max, "{} key is larger than max key {}", tzk, max);
        found_max = found_max || tzk.get_key() == max;
    }
    assert!(found_max, "Did not find a time zone with the MAX_TIME_ZONE_KEY");
}

#[test]
fn test_zone_key_id_range() {
    let max = TimeZoneKey::max_time_zone_key() as usize;
    let mut has_value = vec![false; max + 1];
    for tzk in TimeZoneKey::get_time_zone_keys() {
        let k = tzk.get_key();
        assert!(k >= 0, "{} has a negative time zone key", tzk);
        assert!(!has_value[k as usize], "Another time zone has the same zone key as {}", tzk);
        has_value[k as usize] = true;
    }
    for gap in [2040usize, 2180, 2186, 2196] {
        assert!(!has_value[gap]);
        has_value[gap] = true;
    }
    for (i, v) in has_value.iter().enumerate() {
        assert!(*v, "There is no time zone with key {}", i);
    }
}

fn fmix64(mut k: u64) -> u64 {
    k ^= k >> 33;
    k = k.wrapping_mul(0xff51afd7ed558ccd);
    k ^= k >> 33;
    k = k.wrapping_mul(0xc4ceb9fe1a85ec53);
    k ^= k >> 33;
    k
}

fn murmur3_128_as_long(data: &[u8]) -> i64 {
    const C1: u64 = 0x87c37b91114253d5;
    const C2: u64 = 0x4cf5ad432745937f;
    let mut h1: u64 = 0;
    let mut h2: u64 = 0;
    let chunks = data.len() / 16;
    for i in 0..chunks {
        let b = &data[i * 16..i * 16 + 16];
        let mut k1 = u64::from_le_bytes(b[0..8].try_into().unwrap());
        let mut k2 = u64::from_le_bytes(b[8..16].try_into().unwrap());
        k1 = k1.wrapping_mul(C1).rotate_left(31).wrapping_mul(C2);
        h1 ^= k1;
        h1 = h1.rotate_left(27).wrapping_add(h2).wrapping_mul(5).wrapping_add(0x52dce729);
        k2 = k2.wrapping_mul(C2).rotate_left(33).wrapping_mul(C1);
        h2 ^= k2;
        h2 = h2.rotate_left(31).wrapping_add(h1).wrapping_mul(5).wrapping_add(0x38495ab5);
    }
    let tail = &data[chunks * 16..];
    let mut k1: u64 = 0;
    let mut k2: u64 = 0;
    for (i, b) in tail.iter().enumerate() {
        if i < 8 {
            k1 ^= (*b as u64) << (8 * i);
        } else {
            k2 ^= (*b as u64) << (8 * (i - 8));
        }
    }
    if tail.len() > 8 {
        k2 = k2.wrapping_mul(C2).rotate_left(33).wrapping_mul(C1);
        h2 ^= k2;
    }
    if !tail.is_empty() {
        k1 = k1.wrapping_mul(C1).rotate_left(31).wrapping_mul(C2);
        h1 ^= k1;
    }
    let len = data.len() as u64;
    h1 ^= len;
    h2 ^= len;
    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);
    h1 = fmix64(h1);
    h2 = fmix64(h2);
    h1 = h1.wrapping_add(h2);
    h1 as i64
}

#[test]
fn test_zone_key_data() {
    let mut keys = TimeZoneKey::get_time_zone_keys();
    keys.sort_by_key(|k| k.get_key());
    keys.dedup_by_key(|k| k.get_key());
    let mut bytes = Vec::new();
    for tzk in keys {
        bytes.extend_from_slice(&tzk.get_key().to_le_bytes());
        bytes.extend_from_slice(tzk.get_id().as_bytes());
    }
    assert_eq!(murmur3_128_as_long(&bytes), -4582158485614614451, "zone-index.properties file contents changed!");
}
