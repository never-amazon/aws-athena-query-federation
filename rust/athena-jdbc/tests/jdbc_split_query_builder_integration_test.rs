mod jdbc_support;

use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::FieldRef;
use bigdecimal::BigDecimal;
use indexmap::IndexMap;

use athena_federation_sdk::data::ArrowType;
use athena_federation_sdk::domain::predicate::functions::FunctionName;
use athena_federation_sdk::domain::predicate::{Constraints, QueryPlan, DEFAULT_NO_LIMIT};
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::Result;
use athena_federation_substrait::SqlDialect;
use athena_jdbc::manager::{
    FederationExpressionParser, JdbcSplitQueryBuilder, JdbcSplitQueryBuilderBase, TypeAndValue,
};
use athena_jdbc::sql::{JdbcConnection, SqlException, SqlValue};

use jdbc_support::{FakeConnection, ResultSetData};

const QUOTE_CHAR: &str = "\"";

const EMPLOYEE_PLAN: &str = "GrgEErUECoMEOoAECggSBgoEFBUWFxLFAwrCAwoCCgAS2gIKC2VtcGxveWVlX2lkCglpc19hY3RpdmUKDWVtcGxveWVlX25hbWUKCWpvYl90aXRsZQoHYWRkcmVzcwoJam9pbl9kYXRlCgl0aW1lc3RhbXAKCGR1cmF0aW9uCgZzYWxhcnkKBWJvbnVzCgVoYXNoMQoFaGFzaDIKBGNvZGUKBWRlYml0CgVjb3VudAoGYW1vdW50CgdiYWxhbmNlCgRyYXRlCgpkaWZmZXJlbmNlCg5wYXJ0aXRpb25fbmFtZRKYAQoHugEECAEYAQoEOgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoFggECEAEKBYoCAhgBCgRiAhABCgnCAQYIBBATIAEKCcIBBggEEBMgAQoEOgIQAQoEOgIQAQoEOgIQAQoJwgEGCAMQEyABCgQ6AhABCgnCAQYIAxATIAEKBDoCEAEKCcIBBggDEBMgAQoEOgIQAQoEYgIQARgCOl8KMU9SQUNMRV9CQVNJQ19EQlRBQkxFX1NDSEVNQV9HQU1NQV9FVV9XRVNUXzFfSU5URUcKKk9SQUNMRV9CQVNJQ19EQlRBQkxFX0dBTU1BX0VVX1dFU1RfMV9JTlRFRxoIEgYKAhIAIgAaChIICgQSAggCIgAaChIICgQSAggDIgAaChIICgQSAggIIgASC2VtcGxveWVlX2lkEg1lbXBsb3llZV9uYW1lEglqb2JfdGl0bGUSBnNhbGFyeTILEEoqB2lzdGhtdXM=";
const FILTERED_PLAN: &str = "ChsIARIXL2Z1bmN0aW9uc19ib29sZWFuLnlhbWwKHggCEhovZnVuY3Rpb25zX2NvbXBhcmlzb24ueWFtbBIOGgwIARoIYW5kOmJvb2wSFRoTCAIQARoNZXF1YWw6YW55X2FueRISGhAIAhACGgpndDphbnlfYW55Eg8aDQgBEAMaB29yOmJvb2wa8w4S8A4K7Q466g4KCRIHCgUIBAECAxKgDhKdDgoCCgAS2QEK1gEKAgoAEqQBCgJpZAoEbmFtZQoDYWdlCgZzYWxhcnkKCmRlcGFydG1lbnQKCWhpcmVfZGF0ZQoJaXNfYWN0aXZlCgVzY29yZQoEY2l0eQoHY291bnRyeQoHcGFydF9pZBJKCgQqAhABCgRiAhABCgQqAhABCgnCAQYIAhAKIAEKBGICEAEKBYIBAhABCgQKAhABCgRaAhABCgRiAhABCgRiAhABCgRiAhABGAI6KQoKZ2x1ZV90ZWFtMgobdGVzdF90YWJsZV9xdWVyeV9mZWRlcmF0aW9uGroMGrcMGgQKAhABIqMMGqAMGp0MGgQKAhABIusJGugJGuUJGgQKAhABIm8abRprGgQKAhABIiUaIxohCAEaBAoCEAEiDBoKEggKBBICCAYiACIJGgcKBQgBkAMBIjwaOho4CAIaBAoCEAEiDBoKEggKBBICCAMiACIgGh4KHMIBFgoQwM9qAAAAAAAAAAAAAAAAABAHGAKQAwEi6wga6Aga5QgIAxoECgIQASKsCBqpCBqmCAgDGgQKAhABIucHGuQHGuEHCAMaBAoCEAEipgcaowcaoAcIAxoECgIQASLmBhrjBhrgBggDGgQKAhABIqcGGqQGGqEGCAMaBAoCEAEi5wUa5AUa4QUIAxoECgIQASKmBRqjBRqgBQgDGgQKAhABIuMEGuAEGt0ECAMaBAoCEAEiogQanwQanAQIAxoECgIQASLfAxrcAxrZAwgDGgQKAhABIqADGp0DGpoDCAMaBAoCEAEi4QIa3gIa2wIIAxoECgIQASKjAhqgAhqdAggDGgQKAhABIuUBGuIBGt8BCAMaBAoCEAEipgEaowEaoAEIAxoECgIQASJmGmQaYggDGgQKAhABIisaKRonCAEaBAoCEAEiDBoKEggKBBICCAgiACIPGg0KC2IGQXVzdGlukAMBIisaKRonCAEaBAoCEAEiDBoKEggKBBICCAgiACIPGg0KC2IGQm9zdG9ukAMBIi4aLBoqCAEaBAoCEAEiDBoKEggKBBICCAgiACISGhAKDmIJQ2hhcmxvdHRlkAMBIiwaKhooCAEaBAoCEAEiDBoKEggKBBICCAgiACIQGg4KDGIHQ2hpY2Fnb5ADASIrGikaJwgBGgQKAhABIgwaChIICgQSAggIIgAiDxoNCgtiBkRhbGxhc5ADASIrGikaJwgBGgQKAhABIgwaChIICgQSAggIIgAiDxoNCgtiBkRlbnZlcpADASIsGioaKAgBGgQKAhABIgwaChIICgQSAggIIgAiEBoOCgxiB0RldHJvaXSQAwEiLBoqGigIARoECgIQASIMGgoSCAoEEgIICCIAIhAaDgoMYgdIb3VzdG9ukAMBIjAaLhosCAEaBAoCEAEiDBoKEggKBBICCAgiACIUGhIKEGILS2Fuc2FzIENpdHmQAwEiLhosGioIARoECgIQASIMGgoSCAoEEgIICCIAIhIaEAoOYglMYXMgVmVnYXOQAwEiMBouGiwIARoECgIQASIMGgoSCAoEEgIICCIAIhQaEgoQYgtMb3MgQW5nZWxlc5ADASIuGiwaKggBGgQKAhABIgwaChIICgQSAggIIgAiEhoQCg5iCU5hc2h2aWxsZZADASItGisaKQgBGgQKAhABIgwaChIICgQSAggIIgAiERoPCg1iCE5ldyBZb3JrkAMBIiwaKhooCAEaBAoCEAEiDBoKEggKBBICCAgiACIQGg4KDGIHUGhvZW5peJADASItGisaKQgBGgQKAhABIgwaChIICgQSAggIIgAiERoPCg1iCFBvcnRsYW5kkAMBIi4aLBoqCAEaBAoCEAEiDBoKEggKBBICCAgiACISGhAKDmIJU2FuIERpZWdvkAMBIjIaMBouCAEaBAoCEAEiDBoKEggKBBICCAgiACIWGhQKEmINU2FuIEZyYW5jaXNjb5ADASIsGioaKAgBGgQKAhABIgwaChIICgQSAggIIgAiEBoOCgxiB1NlYXR0bGWQAwEipgIaowIaoAIIAxoECgIQASLpARrmARrjAQgDGgQKAhABIqgBGqUBGqIBCAMaBAoCEAEiZxplGmMIAxoECgIQASIwGi4aLAgBGgQKAhABIgwaChIICgQSAggEIgAiFBoSChBiC0VuZ2luZWVyaW5nkAMBIicaJRojCAEaBAoCEAEiDBoKEggKBBICCAQiACILGgkKB2ICSFKQAwEiLxotGisIARoECgIQASIMGgoSCAoEEgIIBCIAIhMaEQoPYgpNYW5hZ2VtZW50kAMBIi4aLBoqCAEaBAoCEAEiDBoKEggKBBICCAQiACISGhAKDmIJTWFya2V0aW5nkAMBIioaKBomCAEaBAoCEAEiDBoKEggKBBICCAQiACIOGgwKCmIFU2FsZXOQAwEiCRoHCgUIAZADARoKEggKBBICCAgiABoKEggKBBICCAQiABoKEggKBBICCAEiABoKEggKBBICCAIiABoKEggKBBICCAMiAA==";
const AGE_AND_ACTIVE_PLAN: &str = "ChsIARIXL2Z1bmN0aW9uc19ib29sZWFuLnlhbWwKHggCEhovZnVuY3Rpb25zX2NvbXBhcmlzb24ueWFtbBIOGgwIARoIYW5kOmJvb2wSFRoTCAIQARoNZXF1YWw6YW55X2FueRISGhAIAhACGgpndDphbnlfYW55GuYCEuMCCuACGt0CCgIKABLSAhLPAgoCCgAS2QEK1gEKAgoAEqQBCgJpZAoEbmFtZQoDYWdlCgZzYWxhcnkKCmRlcGFydG1lbnQKCWhpcmVfZGF0ZQoJaXNfYWN0aXZlCgVzY29yZQoEY2l0eQoHY291bnRyeQoHcGFydF9pZBJKCgQqAhABCgRiAhABCgQqAhABCgnCAQYIAhAKIAEKBGICEAEKBYIBAhABCgQKAhABCgRaAhABCgRiAhABCgRiAhABCgRiAhABGAI6KQoKZ2x1ZV90ZWFtMgobdGVzdF90YWJsZV9xdWVyeV9mZWRlcmF0aW9uGm0aaxoECgIQASJYGlYaVBoECgIQASIlGiMaIQgBGgQKAhABIgwaChIICgQSAggGIgAiCRoHCgUIAJADASIlGiMaIQgCGgQKAhABIgwaChIICgQSAggCIgAiCRoHCgUoHpADASIJGgcKBQgBkAMBGAAgZA==";

const PLANS: [&str; 12] = [
    "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CAEaBAoCEAEiDBoKEggKBBICCAQiACILGgkKB2ICSFKQAwEiLxotGisIARoECgIQASIMGgoSCAoEEgIIBCIAIhMaEQoPYgpNYW5hZ2VtZW50kAMBIi4aLBoqCAEaBAoCEAEiDBoKEggKBBICCAQiACISGhAKDmIJTWFya2V0aW5nkAMBIioaKBomCAEaBAoCEAEiDBoKEggKBBICCAQiACIOGgwKCmIFU2FsZXOQAwEiCRoHCgUIAZADARoKEggKBBICCAgiABoKEggKBBICCAQiABoKEggKBBICCAEiABoKEggKBBICCAIiABoKEggKBBICCAMiAA==",
    "ChsIARIXL2Z1bmN0aW9uc19ib29sZWFuLnlhbWwKHggCEhovZnVuY3Rpb25zX2NvbXBhcmlzb24ueWFtbBIOGgwIARoIYW5kOmJvb2wSFRoTCAIQARoNZXF1YWw6YW55X2FueRISGhAIAhACGgpndDphbnlfYW55GuYCEuMCCuACGt0CCgIKABLSAhLPAgoCCgAS2QEK1gEKAgoAEqQBCgJpZAoEbmFtZQoDYWdlCgZzYWxhcnkKCmRlcGFydG1lbnQKCWhpcmVfZGF0ZQoJaXNfYWN0aXZlCgVzY29yZQoEY2l0eQoHY291bnRyeQoHcGFydF9pZBJKCgQqAhABCgRiAhABCgQqAhABCgnCAQYIAhAKIAEKBGICEAEKBYIBAhABCgQKAhABCgRaAhABCgRiAhABCgRiAhABCgRiAhABGAI6KQoKZ2x1ZV90ZWFtMgobdGVzdF90YWJsZV9xdWVyeV9mZWRlcmF0aW9uGm0aaxoECgIQASJYGlYaVBoECgIQASIlGiMaIQgBGgQKAhABIgwaChIICgQSAggGIgAiCRoHCgUIAJADASIlGiMaIQgCGgQKAhABIgwaChIICgQSAggCIgAiCRoHCgUoHpADASIJGgcKBQgBkAMBGAAgZA==",
    "GucFEuQFCuEFGt4FCgIKABLTBSrQBQoCCgASuQUKtgUKAgoAEpUFChFjY19jYWxsX2NlbnRlcl9zawoRY2NfY2FsbF9jZW50ZXJfaWQKEWNjX3JlY19zdGFydF9kYXRlCg9jY19yZWNfZW5kX2RhdGUKEWNjX2Nsb3NlZF9kYXRlX3NrCg9jY19vcGVuX2RhdGVfc2sKB2NjX25hbWUKCGNjX2NsYXNzCgxjY19lbXBsb3llZXMKCGNjX3NxX2Z0CghjY19ob3VycwoKY2NfbWFuYWdlcgoJY2NfbWt0X2lkCgxjY19ta3RfY2xhc3MKC2NjX21rdF9kZXNjChFjY19tYXJrZXRfbWFuYWdlcgoLY2NfZGl2aXNpb24KEGNjX2RpdmlzaW9uX25hbWUKCmNjX2NvbXBhbnkKD2NjX2NvbXBhbnlfbmFtZQoQY2Nfc3RyZWV0X251bWJlcgoOY2Nfc3RyZWV0X25hbWUKDmNjX3N0cmVldF90eXBlCg9jY19zdWl0ZV9udW1iZXIKB2NjX2NpdHkKCWNjX2NvdW50eQoIY2Nfc3RhdGUKBmNjX3ppcAoKY2NfY291bnRyeQoNY2NfZ210X29mZnNldAoRY2NfdGF4X3BlcmNlbnRhZ2UKB3BhcnRfaWQSzgEKBCoCEAEKBGICEAEKBYIBAhABCgWCAQIQAQoEKgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEKgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEKgIQAQoEYgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoJwgEGCAIQBSABCgnCAQYIAhAFIAEKBGICEAEYAjoYCgl3YXJlaG91c2UKC2NhbGxfY2VudGVyGg4KChIICgQSAggGIgAQAhgAIGQ=",
    "ChsIARIXL2Z1bmN0aW9uc19ib29sZWFuLnlhbWwKHggCEhovZnVuY3Rpb25zX2NvbXBhcmlzb24ueWFtbBIOGgwIARoIYW5kOmJvb2wSFRoTCAIQARoNZXF1YWw6YW55X2FueRqzAhKwAgqtAhqqAgoCCgASnwISnAIKAgoAEtkBCtYBCgIKABKkAQoCaWQKBG5hbWUKA2FnZQoGc2FsYXJ5CgpkZXBhcnRtZW50CgloaXJlX2RhdGUKCWlzX2FjdGl2ZQoFc2NvcmUKBGNpdHkKB2NvdW50cnkKB3BhcnRfaWQSSgoEKgIQAQoEYgIQAQoEKgIQAQoJwgEGCAIQCiABCgRiAhABCgWCAQIQAQoECgIQAQoEWgIQAQoEYgIQAQoEYgIQAQoEYgIQARgCOikKCmdsdWVfdGVhbTIKG3Rlc3RfdGFibGVfcXVlcnlfZmVkZXJhdGlvbho6GjgaBAoCEAEiJRojGiEIARoECgIQASIMGgoSCAoEEgIIBiIAIgkaBwoFCAGQAwEiCRoHCgUIAZADARgAIAU=",
    "Go8BEowBCokBCoYBCgIKABJbCglkZXB0X25hbWUKDG1hbmFnZXJfbmFtZQoGYnVkZ2V0Cghsb2NhdGlvbgoHcGFydF9pZBIlCgRiAhABCgRiAhABCgnCAQYIAhAMIAEKBGICEAEKBGICEAEYAjojCgpnbHVlX3RlYW0yChV0ZXN0X3RhYmxlX2RlcGFydG1lbnQ=",
    "GucFEuQFCuEFGt4FCgIKABLTBSrQBQoCCgASuQUKtgUKAgoAEpUFChFjY19jYWxsX2NlbnRlcl9zawoRY2NfY2FsbF9jZW50ZXJfaWQKEWNjX3JlY19zdGFydF9kYXRlCg9jY19yZWNfZW5kX2RhdGUKEWNjX2Nsb3NlZF9kYXRlX3NrCg9jY19vcGVuX2RhdGVfc2sKB2NjX25hbWUKCGNjX2NsYXNzCgxjY19lbXBsb3llZXMKCGNjX3NxX2Z0CghjY19ob3VycwoKY2NfbWFuYWdlcgoJY2NfbWt0X2lkCgxjY19ta3RfY2xhc3MKC2NjX21rdF9kZXNjChFjY19tYXJrZXRfbWFuYWdlcgoLY2NfZGl2aXNpb24KEGNjX2RpdmlzaW9uX25hbWUKCmNjX2NvbXBhbnkKD2NjX2NvbXBhbnlfbmFtZQoQY2Nfc3RyZWV0X251bWJlcgoOY2Nfc3RyZWV0X25hbWUKDmNjX3N0cmVldF90eXBlCg9jY19zdWl0ZV9udW1iZXIKB2NjX2NpdHkKCWNjX2NvdW50eQoIY2Nfc3RhdGUKBmNjX3ppcAoKY2NfY291bnRyeQoNY2NfZ210X29mZnNldAoRY2NfdGF4X3BlcmNlbnRhZ2UKB3BhcnRfaWQSzgEKBCoCEAEKBGICEAEKBYIBAhABCgWCAQIQAQoEKgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEKgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEKgIQAQoEYgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoJwgEGCAIQBSABCgnCAQYIAhAFIAEKBGICEAEYAjoYCgl3YXJlaG91c2UKC2NhbGxfY2VudGVyGg4KChIICgQSAggCIgAQAhgAIGQ=",
    "ChsIARIXL2Z1bmN0aW9uc19ib29sZWFuLnlhbWwKHggCEhovZnVuY3Rpb25zX2NvbXBhcmlzb24ueWFtbBIOGgwIARoIYW5kOmJvb2wSEhoQCAIQARoKZ3Q6YW55X2FueRIVGhMIAhACGg1lcXVhbDphbnlfYW55GuYCEuMCCuACGt0CCgIKABLSAhLPAgoCCgAS2QEK1gEKAgoAEqQBCgJpZAoEbmFtZQoDYWdlCgZzYWxhcnkKCmRlcGFydG1lbnQKCWhpcmVfZGF0ZQoJaXNfYWN0aXZlCgVzY29yZQoEY2l0eQoHY291bnRyeQoHcGFydF9pZBJKCgQqAhABCgRiAhABCgQqAhABCgnCAQYIAhAKIAEKBGICEAEKBYIBAhABCgQKAhABCgRaAhABCgRiAhABCgRiAhABCgRiAhABGAI6KQoKZ2x1ZV90ZWFtMgobdGVzdF90YWJsZV9xdWVyeV9mZWRlcmF0aW9uGm0aaxoECgIQASJYGlYaVBoECgIQASIlGiMaIQgBGgQKAhABIgwaChIICgQSAggCIgAiCRoHCgUoIJADASIlGiMaIQgCGgQKAhABIgwaChIICgQSAggGIgAiCRoHCgUIAZADASIJGgcKBQgBkAMBGAAgBQ==",
    "GucFEuQFCuEFGt4FCgIKABLTBSrQBQoCCgASuQUKtgUKAgoAEpUFChFjY19jYWxsX2NlbnRlcl9zawoRY2NfY2FsbF9jZW50ZXJfaWQKEWNjX3JlY19zdGFydF9kYXRlCg9jY19yZWNfZW5kX2RhdGUKEWNjX2Nsb3NlZF9kYXRlX3NrCg9jY19vcGVuX2RhdGVfc2sKB2NjX25hbWUKCGNjX2NsYXNzCgxjY19lbXBsb3llZXMKCGNjX3NxX2Z0CghjY19ob3VycwoKY2NfbWFuYWdlcgoJY2NfbWt0X2lkCgxjY19ta3RfY2xhc3MKC2NjX21rdF9kZXNjChFjY19tYXJrZXRfbWFuYWdlcgoLY2NfZGl2aXNpb24KEGNjX2RpdmlzaW9uX25hbWUKCmNjX2NvbXBhbnkKD2NjX2NvbXBhbnlfbmFtZQoQY2Nfc3RyZWV0X251bWJlcgoOY2Nfc3RyZWV0X25hbWUKDmNjX3N0cmVldF90eXBlCg9jY19zdWl0ZV9udW1iZXIKB2NjX2NpdHkKCWNjX2NvdW50eQoIY2Nfc3RhdGUKBmNjX3ppcAoKY2NfY291bnRyeQoNY2NfZ210X29mZnNldAoRY2NfdGF4X3BlcmNlbnRhZ2UKB3BhcnRfaWQSzgEKBCoCEAEKBGICEAEKBYIBAhABCgWCAQIQAQoEKgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEKgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEKgIQAQoEYgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoJwgEGCAIQBSABCgnCAQYIAhAFIAEKBGICEAEYAjoYCgl3YXJlaG91c2UKC2NhbGxfY2VudGVyGg4KChIICgQSAggCIgAQAhgAIGQ=",
    "GpMCEpACCo0CGooCCgIKABL/ASr8AQoCCgAS5QEK4gEKAgoAEscBCgpvX29yZGVya2V5CglvX2N1c3RrZXkKDW9fb3JkZXJzdGF0dXMKDG9fdG90YWxwcmljZQoLb19vcmRlcmRhdGUKD29fb3JkZXJwcmlvcml0eQoHb19jbGVyawoOb19zaGlwcHJpb3JpdHkKCW9fY29tbWVudAoJcGFydGl0aW9uEkQKBDoCEAEKBDoCEAEKBGICEAEKCcIBBggCEAwgAQoFggECEAEKBGICEAEKBGICEAEKBDoCEAEKBGICEAEKBGICEAEYAjoSCgh0cGNoX3NmMQoGb3JkZXJzGg4KChIICgQSAggEIgAQAhgAIGQ=",
];

struct EmptyExpressionParser;

impl FederationExpressionParser for EmptyExpressionParser {
    fn map_function_to_data_source_syntax(
        &self,
        _function_name: &FunctionName,
        _arrow_type: &ArrowType,
        _arguments: &[String],
    ) -> Result<String> {
        Ok(String::new())
    }

    fn parse_complex_expressions(
        &self,
        _columns: &[FieldRef],
        _constraints: &Constraints,
        _accumulator: &mut Vec<TypeAndValue>,
    ) -> Result<Vec<String>> {
        Ok(Vec::new())
    }
}

struct TestBuilder {
    base: JdbcSplitQueryBuilderBase,
}

impl JdbcSplitQueryBuilder for TestBuilder {
    fn query_builder_base(&self) -> &JdbcSplitQueryBuilderBase {
        &self.base
    }

    fn get_from_clause_with_split(
        &self,
        _catalog: &str,
        _schema: &str,
        table: &str,
        _split: &Split,
    ) -> String {
        format!(" FROM {}", self.quote(table))
    }

    fn get_partition_where_clauses(&self, _split: &Split) -> Vec<String> {
        Vec::new()
    }
}

fn builder() -> TestBuilder {
    TestBuilder {
        base: JdbcSplitQueryBuilderBase::new_with_federation_expression_parser(
            QUOTE_CHAR,
            Arc::new(EmptyExpressionParser),
        ),
    }
}

fn plan_constraints(plan: &str) -> Constraints {
    Constraints::new(
        IndexMap::new(),
        Vec::new(),
        Vec::new(),
        DEFAULT_NO_LIMIT,
        HashMap::new(),
        Some(QueryPlan::new(None, plan)),
    )
}

fn split() -> Split {
    Split::new(None, None, HashMap::new())
}

fn connection_with_rows(rows: usize) -> FakeConnection {
    let connection = FakeConnection::new();
    connection.on_query(move |_| {
        let data: Vec<Vec<&str>> = (0..rows).map(|_| vec!["1"]).collect();
        let refs: Vec<&[&str]> = data.iter().map(Vec::as_slice).collect();
        Ok(ResultSetData::of_strings(&["c"], &refs))
    });
    connection
}

#[tokio::test]
async fn test_real_prepared_statement_with_multiple_parameters() {
    let connection = connection_with_rows(1);
    let statement = builder()
        .prepare_statement_with_calcite_sql(
            &connection,
            &plan_constraints(AGE_AND_ACTIVE_PLAN),
            &SqlDialect::ansi(),
            &split(),
        )
        .await
        .unwrap();
    assert_eq!(
        statement.sql(),
        "SELECT *\nFROM `glue_team2`.`test_table_query_federation`\nWHERE `age` > ? AND `is_active` = ?\nFETCH NEXT 100 ROWS ONLY"
    );
    assert_eq!(
        statement.parameters(),
        &[SqlValue::Integer(30), SqlValue::Boolean(false)]
    );

    let filtered = builder()
        .prepare_statement_with_calcite_sql(
            &connection,
            &plan_constraints(FILTERED_PLAN),
            &SqlDialect::ansi(),
            &split(),
        )
        .await
        .unwrap();
    assert_eq!(filtered.parameter(1), Some(&SqlValue::Boolean(true)));
    assert_eq!(
        filtered.parameter(2),
        Some(&SqlValue::Decimal(
            "70000.00".parse::<BigDecimal>().unwrap()
        ))
    );
    assert_eq!(
        filtered.parameter(3),
        Some(&SqlValue::String("Austin".to_string()))
    );
    assert_eq!(
        filtered.parameters().len(),
        filtered.sql().matches('?').count()
    );

    let mut result_set = connection.execute_query(&statement).await.unwrap();
    assert!(result_set.next().await.unwrap());
}

#[tokio::test]
async fn test_real_prepared_statement_with_calcite_sql() {
    let connection = connection_with_rows(0);
    let statement = builder()
        .prepare_statement_with_calcite_sql(
            &connection,
            &plan_constraints(EMPLOYEE_PLAN),
            &SqlDialect::ansi(),
            &split(),
        )
        .await
        .unwrap();
    assert!(statement
        .sql()
        .starts_with("SELECT `employee_id` AS `employee_id0`"));
    assert!(statement.parameters().is_empty());
    let mut result_set = connection.execute_query(&statement).await.unwrap();
    assert!(!result_set.next().await.unwrap());
}

#[tokio::test]
async fn test_real_prepared_statement_with_calcite_sql_parameterized() {
    for plan in PLANS {
        let connection = connection_with_rows(3);
        let statement = builder()
            .prepare_statement_with_calcite_sql(
                &connection,
                &plan_constraints(plan),
                &SqlDialect::ansi(),
                &split(),
            )
            .await
            .unwrap_or_else(|e| panic!("plan {plan} failed: {e:?}"));
        assert!(statement.sql().starts_with("SELECT "));
        assert!(!statement.sql().contains("partition_col"));
        assert_eq!(
            statement.parameters().len(),
            statement.sql().matches('?').count()
        );
        let mut result_set = connection.execute_query(&statement).await.unwrap();
        let mut row_count = 0;
        while result_set.next().await.unwrap() {
            row_count += 1;
        }
        assert_eq!(row_count, 3);
    }
}

#[tokio::test]
async fn test_real_prepared_statement_with_calcite_sql_error_cases() {
    let connection = FakeConnection::new();
    let error = builder()
        .prepare_statement_with_calcite_sql(
            &connection,
            &plan_constraints("SELECT * FORM test_table"),
            &SqlDialect::ansi(),
            &split(),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Failed to prepare statement with Calcite"
    );
    assert!(connection.prepared_sql().is_empty());
}

#[tokio::test]
async fn test_real_prepared_statement_with_calcite_sql_invalid_column() {
    let connection = FakeConnection::new();
    connection.on_query(|_| Err(SqlException::new("Column \"INVALID_COLUMN\" not found")));
    let statement = builder()
        .prepare_statement_with_calcite_sql(
            &connection,
            &plan_constraints(EMPLOYEE_PLAN),
            &SqlDialect::ansi(),
            &split(),
        )
        .await
        .unwrap();
    let error = connection.execute_query(&statement).await.unwrap_err();
    let message = error.message().to_lowercase();
    assert!(
        message.contains("column") || message.contains("not found") || message.contains("invalid")
    );
}
