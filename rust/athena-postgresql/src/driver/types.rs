use std::collections::HashMap;

use athena_jdbc::sql::JdbcType;

pub(crate) mod oid {
    pub(crate) const BOOL: u32 = 16;
    pub(crate) const BYTEA: u32 = 17;
    pub(crate) const CHAR: u32 = 18;
    pub(crate) const NAME: u32 = 19;
    pub(crate) const INT8: u32 = 20;
    pub(crate) const INT2: u32 = 21;
    pub(crate) const INT4: u32 = 23;
    pub(crate) const TEXT: u32 = 25;
    pub(crate) const OID: u32 = 26;
    pub(crate) const JSON: u32 = 114;
    pub(crate) const POINT: u32 = 600;
    pub(crate) const BOX: u32 = 603;
    pub(crate) const FLOAT4: u32 = 700;
    pub(crate) const FLOAT8: u32 = 701;
    pub(crate) const MONEY: u32 = 790;
    pub(crate) const BPCHAR: u32 = 1042;
    pub(crate) const VARCHAR: u32 = 1043;
    pub(crate) const DATE: u32 = 1082;
    pub(crate) const TIME: u32 = 1083;
    pub(crate) const TIMESTAMP: u32 = 1114;
    pub(crate) const TIMESTAMPTZ: u32 = 1184;
    pub(crate) const INTERVAL: u32 = 1186;
    pub(crate) const TIMETZ: u32 = 1266;
    pub(crate) const BIT: u32 = 1560;
    pub(crate) const VARBIT: u32 = 1562;
    pub(crate) const NUMERIC: u32 = 1700;
    pub(crate) const REFCURSOR: u32 = 1790;
    pub(crate) const JSONB: u32 = 3802;
}

const UNKNOWN_LENGTH: i32 = i32::MAX;

const STATIC_TYPES: [(&str, u32, JdbcType, u32); 26] = [
    ("int2", oid::INT2, JdbcType::SmallInt, 1005),
    ("int4", oid::INT4, JdbcType::Integer, 1007),
    ("oid", oid::OID, JdbcType::BigInt, 1028),
    ("int8", oid::INT8, JdbcType::BigInt, 1016),
    ("money", oid::MONEY, JdbcType::Double, 791),
    ("numeric", oid::NUMERIC, JdbcType::Numeric, 1231),
    ("float4", oid::FLOAT4, JdbcType::Real, 1021),
    ("float8", oid::FLOAT8, JdbcType::Double, 1022),
    ("char", oid::CHAR, JdbcType::Char, 1002),
    ("bpchar", oid::BPCHAR, JdbcType::Char, 1014),
    ("varchar", oid::VARCHAR, JdbcType::Varchar, 1015),
    ("varbit", oid::VARBIT, JdbcType::Other, 1563),
    ("text", oid::TEXT, JdbcType::Varchar, 1009),
    ("name", oid::NAME, JdbcType::Varchar, 1003),
    ("bytea", oid::BYTEA, JdbcType::Binary, 1001),
    ("bool", oid::BOOL, JdbcType::Bit, 1000),
    ("bit", oid::BIT, JdbcType::Bit, 1561),
    ("date", oid::DATE, JdbcType::Date, 1182),
    ("time", oid::TIME, JdbcType::Time, 1183),
    ("timetz", oid::TIMETZ, JdbcType::Time, 1270),
    ("timestamp", oid::TIMESTAMP, JdbcType::Timestamp, 1115),
    ("timestamptz", oid::TIMESTAMPTZ, JdbcType::Timestamp, 1185),
    ("refcursor", oid::REFCURSOR, JdbcType::RefCursor, 2201),
    ("json", oid::JSON, JdbcType::Other, 199),
    ("jsonb", oid::JSONB, JdbcType::Other, 3807),
    ("point", oid::POINT, JdbcType::Other, 1017),
];

const STATIC_BOX: (&str, u32, JdbcType, u32) = ("box", oid::BOX, JdbcType::Other, 1020);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PgType {
    pub(crate) oid: u32,
    pub(crate) name: String,
    pub(crate) sql_type: JdbcType,
    pub(crate) element: Option<u32>,
    pub(crate) delimiter: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LookedUpType {
    pub(crate) oid: u32,
    pub(crate) name: String,
    pub(crate) type_type: String,
    pub(crate) is_array: bool,
    pub(crate) element: u32,
    pub(crate) delimiter: u8,
    pub(crate) on_path: bool,
    pub(crate) schema: String,
}

pub(crate) const TYPE_LOOKUP_QUERY: &str = "SELECT t.oid, t.typname, t.typtype, t.typinput = 'pg_catalog.array_in'::regproc AS is_array, t.typelem, t.typdelim, n.nspname = ANY(current_schemas(true)) AS on_path, n.nspname FROM pg_catalog.pg_type t JOIN pg_catalog.pg_namespace n ON t.typnamespace = n.oid WHERE t.oid = ANY($1::oid[])";

#[derive(Debug, Clone)]
pub(crate) struct TypeCache {
    types: HashMap<u32, PgType>,
}

impl Default for TypeCache {
    fn default() -> TypeCache {
        let mut types = HashMap::new();
        for (name, type_oid, sql_type, array_oid) in STATIC_TYPES.iter().chain([&STATIC_BOX]) {
            let delimiter = if *type_oid == oid::BOX { b';' } else { b',' };
            types.insert(
                *type_oid,
                PgType {
                    oid: *type_oid,
                    name: (*name).to_string(),
                    sql_type: *sql_type,
                    element: None,
                    delimiter,
                },
            );
            types.insert(
                *array_oid,
                PgType {
                    oid: *array_oid,
                    name: format!("_{name}"),
                    sql_type: JdbcType::Array,
                    element: Some(*type_oid),
                    delimiter,
                },
            );
        }
        TypeCache { types }
    }
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

impl TypeCache {
    pub(crate) fn get(&self, type_oid: u32) -> Option<&PgType> {
        self.types.get(&type_oid)
    }

    pub(crate) fn missing<'a, I>(&self, oids: I) -> Vec<u32>
    where
        I: IntoIterator<Item = &'a u32>,
    {
        let mut missing: Vec<u32> = Vec::new();
        for type_oid in oids {
            if *type_oid != 0 && !self.types.contains_key(type_oid) && !missing.contains(type_oid) {
                missing.push(*type_oid);
            }
        }
        missing
    }

    pub(crate) fn missing_elements(&self) -> Vec<u32> {
        self.missing(
            &self
                .types
                .values()
                .filter_map(|t| t.element)
                .collect::<Vec<_>>(),
        )
    }

    pub(crate) fn insert(&mut self, looked_up: LookedUpType) {
        let sql_type = if looked_up.is_array {
            JdbcType::Array
        } else {
            match looked_up.type_type.as_str() {
                "c" => JdbcType::Struct,
                "d" => JdbcType::Distinct,
                "e" => JdbcType::Varchar,
                _ => JdbcType::Other,
            }
        };
        let name = if looked_up.on_path {
            looked_up.name
        } else {
            format!("{}.{}", quoted(&looked_up.schema), quoted(&looked_up.name))
        };
        self.types.insert(
            looked_up.oid,
            PgType {
                oid: looked_up.oid,
                name,
                sql_type,
                element: (looked_up.is_array && looked_up.element != 0)
                    .then_some(looked_up.element),
                delimiter: looked_up.delimiter,
            },
        );
    }

    pub(crate) fn insert_unknown(&mut self, type_oid: u32) {
        self.types.entry(type_oid).or_insert_with(|| PgType {
            oid: type_oid,
            name: type_oid.to_string(),
            sql_type: JdbcType::Other,
            element: None,
            delimiter: b',',
        });
    }

    pub(crate) fn sql_type(&self, type_oid: u32) -> JdbcType {
        self.get(type_oid).map_or(JdbcType::Other, |t| t.sql_type)
    }

    pub(crate) fn pg_type_name(&self, type_oid: u32) -> String {
        self.get(type_oid)
            .map_or_else(|| type_oid.to_string(), |t| t.name.clone())
    }

    fn base_oid(&self, type_oid: u32) -> u32 {
        self.get(type_oid)
            .and_then(|t| t.element)
            .unwrap_or(type_oid)
    }

    pub(crate) fn precision(&self, type_oid: u32, typmod: i32) -> i32 {
        let base = self.base_oid(type_oid);
        match base {
            oid::INT2 => 5,
            oid::OID | oid::INT4 => 10,
            oid::INT8 => 19,
            oid::FLOAT4 => 8,
            oid::FLOAT8 => 17,
            oid::NUMERIC => {
                if typmod == -1 {
                    0
                } else {
                    ((typmod - 4) >> 16) & 0xFFFF
                }
            }
            oid::CHAR | oid::BOOL => 1,
            oid::BPCHAR | oid::VARCHAR => {
                if typmod == -1 {
                    UNKNOWN_LENGTH
                } else {
                    typmod - 4
                }
            }
            oid::DATE
            | oid::TIME
            | oid::TIMETZ
            | oid::INTERVAL
            | oid::TIMESTAMP
            | oid::TIMESTAMPTZ => self.display_size(base, typmod),
            oid::BIT => typmod,
            oid::VARBIT => {
                if typmod == -1 {
                    UNKNOWN_LENGTH
                } else {
                    typmod
                }
            }
            _ => UNKNOWN_LENGTH,
        }
    }

    pub(crate) fn scale(&self, type_oid: u32, typmod: i32) -> i32 {
        match self.base_oid(type_oid) {
            oid::FLOAT4 => 8,
            oid::FLOAT8 => 17,
            oid::NUMERIC => {
                if typmod == -1 {
                    0
                } else {
                    (typmod - 4) & 0xFFFF
                }
            }
            oid::TIME | oid::TIMETZ | oid::TIMESTAMP | oid::TIMESTAMPTZ => {
                if typmod == -1 {
                    6
                } else {
                    typmod
                }
            }
            oid::INTERVAL => {
                if typmod == -1 {
                    6
                } else {
                    typmod & 0xFFFF
                }
            }
            _ => 0,
        }
    }

    pub(crate) fn display_size(&self, type_oid: u32, typmod: i32) -> i32 {
        let base = self.base_oid(type_oid);
        match base {
            oid::INT2 => 6,
            oid::INT4 => 11,
            oid::OID => 10,
            oid::INT8 => 20,
            oid::FLOAT4 => 15,
            oid::FLOAT8 => 25,
            oid::CHAR | oid::BOOL => 1,
            oid::DATE => 13,
            oid::TIME | oid::TIMETZ | oid::TIMESTAMP | oid::TIMESTAMPTZ => {
                let second_size = match typmod {
                    -1 => 7,
                    0 => 0,
                    1 => 3,
                    other => other + 1,
                };
                match base {
                    oid::TIME => 8 + second_size,
                    oid::TIMETZ => 8 + second_size + 6,
                    oid::TIMESTAMP => 13 + 1 + 8 + second_size,
                    _ => 13 + 1 + 8 + second_size + 6,
                }
            }
            oid::INTERVAL => 49,
            oid::VARCHAR | oid::BPCHAR => {
                if typmod == -1 {
                    UNKNOWN_LENGTH
                } else {
                    typmod - 4
                }
            }
            oid::NUMERIC => {
                if typmod == -1 {
                    131_089
                } else {
                    let precision = ((typmod - 4) >> 16) & 0xFFFF;
                    let scale = (typmod - 4) & 0xFFFF;
                    1 + precision + i32::from(scale != 0)
                }
            }
            oid::BIT => typmod,
            oid::VARBIT => {
                if typmod == -1 {
                    UNKNOWN_LENGTH
                } else {
                    typmod
                }
            }
            _ => UNKNOWN_LENGTH,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_typmod_gives_precision_scale_and_display_size() {
        let cache = TypeCache::default();
        let typmod = (18 << 16) + 4 + 4;
        assert_eq!(cache.precision(oid::NUMERIC, typmod), 18);
        assert_eq!(cache.scale(oid::NUMERIC, typmod), 4);
        assert_eq!(cache.display_size(oid::NUMERIC, typmod), 20);
        assert_eq!(cache.precision(oid::NUMERIC, -1), 0);
        assert_eq!(cache.display_size(oid::NUMERIC, -1), 131_089);
    }

    #[test]
    fn arrays_use_the_element_type_sizes() {
        let cache = TypeCache::default();
        assert_eq!(cache.sql_type(1007), JdbcType::Array);
        assert_eq!(cache.pg_type_name(1007), "_int4");
        assert_eq!(cache.precision(1007, -1), 10);
        assert_eq!(cache.display_size(1007, -1), 11);
        assert_eq!(cache.precision(1022, -1), 17);
        assert_eq!(cache.scale(1022, -1), 17);
    }

    #[test]
    fn time_types_follow_pgjdbc_sizes() {
        let cache = TypeCache::default();
        assert_eq!(cache.precision(oid::TIME, -1), 15);
        assert_eq!(cache.precision(oid::TIMESTAMP, -1), 29);
        assert_eq!(cache.precision(oid::TIMESTAMPTZ, -1), 35);
        assert_eq!(cache.precision(oid::DATE, -1), 13);
        assert_eq!(cache.scale(oid::TIMESTAMP, -1), 6);
    }

    #[test]
    fn looked_up_types_map_like_pgjdbc() {
        let mut cache = TypeCache::default();
        cache.insert(LookedUpType {
            oid: 2950,
            name: "uuid".to_string(),
            type_type: "b".to_string(),
            is_array: false,
            element: 0,
            delimiter: b',',
            on_path: true,
            schema: "pg_catalog".to_string(),
        });
        cache.insert(LookedUpType {
            oid: 70_000,
            name: "mood".to_string(),
            type_type: "e".to_string(),
            is_array: false,
            element: 0,
            delimiter: b',',
            on_path: false,
            schema: "app".to_string(),
        });
        assert_eq!(cache.sql_type(2950), JdbcType::Other);
        assert_eq!(cache.sql_type(70_000), JdbcType::Varchar);
        assert_eq!(cache.pg_type_name(70_000), "\"app\".\"mood\"");
        assert_eq!(cache.precision(2950, -1), i32::MAX);
    }
}
