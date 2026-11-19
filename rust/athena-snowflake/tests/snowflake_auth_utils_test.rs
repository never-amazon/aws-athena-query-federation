use std::collections::HashMap;

use athena_federation_sdk::exceptions::FederationSourceErrorCode;
use athena_federation_sdk::FederationError;
use athena_snowflake::snowflake_constants;
use athena_snowflake::utils::{SnowflakeAuthType, SnowflakeAuthUtils, SnowflakePrivateKey};
use rsa::pkcs8::DecodePrivateKey;
use rsa::traits::{PrivateKeyParts, PublicKeyParts};
use rsa::RsaPrivateKey;

const TESTUSER: &str = "testuser";
const TESTPASSWORD: &str = "testpassword";
const PASSPHRASE: &str = "test-passphrase";

const VALID_PRIVATE_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----\n\
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCgjDO7+xGLs5bw\n\
0/EKcWRPSoI9PsbgwvFIyQ==\n\
-----END PRIVATE KEY-----";

const ENCRYPTED_PRIVATE_KEY_PEM: &str = "-----BEGIN ENCRYPTED PRIVATE KEY-----\n\
MIIFDjBABgkqhkiG9w0BBQ0wMzAbBgkqhkiG9w0BBQwwDgQIrVKBudJMmggCAggA\n\
MBQGCCqGSIb3DQMHBAgEgw==\n\
-----END ENCRYPTED PRIVATE KEY-----";

const INVALID_ENCRYPTED_PRIVATE_KEY_PEM: &str = "-----BEGIN ENCRYPTED PRIVATE KEY-----\n\
invalid-base64-content\n\
-----END ENCRYPTED PRIVATE KEY-----";

const UNENCRYPTED_PRIVATE_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----\n\
MIIEvwIBADANBgkqhkiG9w0BAQEFAASCBKkwggSlAgEAAoIBAQDd4UYv6P0RH5bD\n\
OAmqJfqQLFH6eGgnRU0M8ef8sjrTHFFyQoxaCuh3aCXk5Nk1tWVhSyBdhCUStVUN\n\
51nmg3KejPziwW19ljvuw9O53aFUhzbYCbWZX5v702CZV3PCWMFubZtz272WL+Ed\n\
afkKmZ3dA8WubyFv+zA0nosualr5B5HJLmIpKPHhEYNY/KslGgJ6gxTuc/uEafAo\n\
wAiq2daiWq4/VU2uoryGcNS2mb6AtoXlCkyTbFglGQ2DwUE+XBOuPghhjgcXxAV+\n\
u4Ct80Hcwxdmalel90axI68Pgog9I92tLMqPuXMB0dlJqXMrm8khiHUWEUD0AiHn\n\
Okgif9FBAgMBAAECggEAK/Rvyhf24kiGvl/AkmN5sswE5YCq6XSyIeleUtIgEyhd\n\
/iuu0Duu99C7CRBI83ne/po6bCw8h+tCUUHOrnrF3yEBDA7TmPiFIPTMYdrFFhC/\n\
TacGwb6OESI71cTcWtjoYO73x8j1UuZjS0duHap2+oy/waGkVyHE1/e8jNJm+UQn\n\
LI1m+SGr9h1QJMw83odtc2At/EJmgGCsXKGf4QOo5vuItEt0ETlSzpyDuo49U1Yc\n\
hS460Q8qwYBb2UOq1TV5/4TP5rvqnHxD3P3Os0fkB31E57Vt5GRnXng2NHl8zWSI\n\
k/DciOZrVmoHhV2+LtuEhqyqKq7shElyKAnasfNfXQKBgQDyux70rv0vpgUyDG7k\n\
R2FUdb0AvU1/QGaSNC4UmZh2FgW63tFWN2IAeHSyCW6fFnRA9H8B+jobOTvd5zlk\n\
jzLcONMjjCXY7Rzl6BdRAFlSrc3awk1ZFz5lzuJa1eZcyPdNlgjD6ukh8Oe4Z+E2\n\
CzahqhYR3gZ4RjVVePrUBZc8bwKBgQDqAlsni9100lbyCes6OLbs3TTyU86gVZ4y\n\
2HjcpkEfWPUlixBkS92IQajhA4jJN0drJcSmwU2cPsGxKrAf3b+uawKYuxEycAD4\n\
zELnO2wBN840/aqO9OU/terfIB7gIg+yNosBEXsmR/XSkthvrAFQW64Tzqq2ofhJ\n\
tbnKNLoFTwKBgQCR1Yj4u3YUvaubcUrizlwU0ZRXsGSyUrANoVoaOFoYgIy6xIqD\n\
QU0ZOUXrfa0/2c7JA9yXBI8Ksi1c2kJqP3rwQxff4KEQnw1guDviB9Ki0RfAGkM8\n\
hrGPm2f16VO5ZWjn7X+gri+Zm9KswYTW4o/VO4p8+6dpdc0vaDtTTXSa8wKBgQDI\n\
VaAIKRs8Kd65c3hn7anc+jubwdfBN8spqF1AOSzDU3r72yP7WV+ciOm1STtPV/3S\n\
YxEJ2a9BthBIRk1l2oeBIKembW/5iB2s4eHc5+g82rO0df4QSOQCORGMV4lXRJG/\n\
diZmkbqmOTDgR0r3v3+YdgR/fIx1shBHukYGMEGYzQKBgQCTtkr2mlbMDMmuaGBd\n\
GbDgGdkEtHjPdRHPxurqk5IUMWaxDyendX7cccmKc2QVkRPNg68ThZs6QKPTxQ5g\n\
Qz94I2WPt0DzBb5KVS6U2cnDp0xUtxBmg8eYkdamISpyL0w8oPIXSXVmlfvljmCl\n\
RlR+O9r/fhM4ThYnZOGaBVpwbg==\n\
-----END PRIVATE KEY-----
";

const ENCRYPTED_PRIVATE_KEY_PEM_PBES2: &str = "-----BEGIN ENCRYPTED PRIVATE KEY-----\n\
MIIFLTBXBgkqhkiG9w0BBQ0wSjApBgkqhkiG9w0BBQwwHAQIOd0338i1gJICAggA\n\
MAwGCCqGSIb3DQIJBQAwHQYJYIZIAWUDBAEqBBB/6GmJysxU/RDBYCFkiweVBIIE\n\
0F58t5lRY7HrPQJRzmqyMq+c2jvXYUDNu4XkIDSs+fPLf/QQUsxmr2300IhsdSwQ\n\
EzQEsxEhjba5STh+jpy/W+i3q9yTOsVCKSVDV4d3k25t1ypvsWi9gCrqXzRonMAu\n\
r0STN3pXNaisyGiohIwbgfOfdhLRIuS3u/BlOYxfGtJgSDq4YV1DQX5J6aXn5HMc\n\
x2xDD6iqOtgZ+fjs81gCg6G2ap+GiRAP8maT2+ZjuDkQeordwm5ub9TmiLWgdmlg\n\
fKxKB1jQou6wb9Ct284ltvik+fipGUxOH1lHSKICdHh4LsC09zgSTNkqHoHaXnL2\n\
lZG5I8mdXJW9/PUo2PnqN7j2fAKTVYWwaRLE72nQv6PgBhkiJYSTJkKgRHDkJJJM\n\
yZL8jg7coa8KAklP8rF/qqgYc7VkDUqNImneS135B2VtXCIvsTgtzYc7ZTNHGHFy\n\
1GlbXj3XGemF4Qkf19T4wilHxIRGzuFjj0kAIyx3PGMIJoQhRKPN5CJ/E2rz3vow\n\
HPQuZ4sAbWEEU4J8b/nHpMGffKb3c/Puem48sX4usYdy4eKEQ12YQgMncD9FK8tq\n\
3craN75TpG5V+Wq53M4wOv+D6twUK/Y8GfefhoOs8+p/6quJuFfFkk9BdQmftlIw\n\
R/F3pE/90g1YyX4ReGZdK/P3HCAUK+giuOPJ0BUdlCc5vq/50kVj/l8D0Efax9gi\n\
YcpU6RgdPPBSu9HWO5EHhMmHkOTkrmBFt/2qWNHlT4i33lg9gVTf2KYt3eNSpmTX\n\
SWKQFBZZhIWoE2mKeRIvl5j4pe6dfb75qFZwa+JImtidcbvaihmg4zaUNzWdxFoV\n\
PbhroOH6I3jv28T8ELMYlfoaexOWFbj1T4RCk2SBP+8+VEowBbkHGeTJWgMHTtlW\n\
bh+Xb/JckDoXoT20kJ9xG6VfZR634vfo0EqgMBVqBl834KQlz/GjmitKpWx3ouPO\n\
ahsoZM0mAFpTcp5ReRBVbuGHMCu0lvp1uTnUZubZf/mkNAnzGgPddGY8xgxm8N7u\n\
MTBrOLjwmjSZ6VvHkjhedPr4F12P6HLfiBLgEj4y5rnr7WkXiAcV5aTauHndxJWo\n\
RGfAetAvvz967mqHA0hNt1v0+TKP3wtB9JqiOACIg7fT4iLSf6XIboT+n+jqkccj\n\
lM3PAfMjzFa09ag2fu1JGejt2fXr3/BOJ9KkZPceqANBej98DysIMOFIQxyLujJ+\n\
DWaPgLo7GhFDT9cgVqaFTSNl3gyCbFWCX/d3Pa7crZhcJ3OzhdJwVvv/ooNXR5Gl\n\
/Chd5bU3/hQg0VoETgfRoOAs27TVxLc94gcXyL+2/V8D+6KU0gwNF0KOTV04xpL2\n\
PkDFF+1RrKRgwEdy2p2jTQY44K29hT8uzeRRcU262J0+CMzqsAtkFoUVOsx2UGgu\n\
Fg2ez6ccdtBwAJHWRcyW2Oz9C7+hhMWvypI1CLQDqFT9lS0T3tAbT0XJzFQRT1Jk\n\
ejwJBpWdkt39VudWP6X0ozycqZ5mfILrMIIYEKAXd2r2U5ZcPQWDzpE3aMzHxaFT\n\
EzP9kpcL69XkBtjiBpP/SjFEFE2pOGxFMTH8Ug+dLK9V/jI1T73fKaUxuvV/qXgr\n\
/Qe8Up46AZGfWKALbN/FWrXOkanMFuPkHcIMdqAjeMuW\n\
-----END ENCRYPTED PRIVATE KEY-----
";

fn credentials(entries: &[(&str, &str)]) -> HashMap<String, String> {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

fn assert_illegal_argument(error: &FederationError, message: &str) {
    assert!(
        matches!(error, FederationError::IllegalArgument(actual) if actual == message),
        "expected IllegalArgument({message}), got {error:?}"
    );
}

fn assert_invalid_private_key(error: &FederationError) {
    assert_eq!("Invalid private key format", error.to_string());
    assert_eq!(
        Some(FederationSourceErrorCode::InvalidInputException),
        error.error_code()
    );
}

fn generated_key() -> RsaPrivateKey {
    RsaPrivateKey::from_pkcs8_pem(UNENCRYPTED_PRIVATE_KEY_PEM).unwrap()
}

fn base64_key() -> String {
    UNENCRYPTED_PRIVATE_KEY_PEM
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect()
}

fn multi_line_pem() -> String {
    UNENCRYPTED_PRIVATE_KEY_PEM.to_string()
}

fn assert_same_key(actual: &SnowflakePrivateKey) {
    let expected = generated_key();
    let actual = RsaPrivateKey::from_pkcs8_der(actual.der()).unwrap();
    assert_eq!(expected.n(), actual.n());
    assert_eq!(expected.d(), actual.d());
}

#[test]
fn test_determine_auth_type_with_key_pair_auth() {
    let credentials = credentials(&[
        (snowflake_constants::SF_USER, TESTUSER),
        (snowflake_constants::PEM_PRIVATE_KEY, VALID_PRIVATE_KEY_PEM),
    ]);
    assert_eq!(
        SnowflakeAuthType::SnowflakeJwt,
        SnowflakeAuthUtils::determine_auth_type(&credentials).unwrap()
    );
}

#[test]
fn test_determine_auth_type_with_o_auth_auth() {
    let credentials = credentials(&[
        (snowflake_constants::USERNAME, TESTUSER),
        (snowflake_constants::AUTH_CODE, "test-auth-code"),
    ]);
    assert_eq!(
        SnowflakeAuthType::Oauth,
        SnowflakeAuthUtils::determine_auth_type(&credentials).unwrap()
    );
}

#[test]
fn test_determine_auth_type_with_password_auth() {
    let credentials = credentials(&[
        (snowflake_constants::USERNAME, TESTUSER),
        (snowflake_constants::PASSWORD, TESTPASSWORD),
    ]);
    assert_eq!(
        SnowflakeAuthType::Snowflake,
        SnowflakeAuthUtils::determine_auth_type(&credentials).unwrap()
    );
}

#[test]
fn test_determine_auth_type_with_null_credentials() {
    let error = SnowflakeAuthUtils::determine_auth_type(&HashMap::new()).unwrap_err();
    assert_illegal_argument(&error, "Credentials cannot be null or empty");
}

#[test]
fn test_determine_auth_type_with_empty_credentials() {
    let error = SnowflakeAuthUtils::determine_auth_type(&credentials(&[])).unwrap_err();
    assert_illegal_argument(&error, "Credentials cannot be null or empty");
}

#[test]
fn test_determine_auth_type_with_empty_private_key() {
    let credentials = credentials(&[
        (snowflake_constants::USERNAME, TESTUSER),
        (snowflake_constants::PRIVATE_KEY, ""),
        (snowflake_constants::PASSWORD, TESTPASSWORD),
    ]);
    assert_eq!(
        SnowflakeAuthType::Snowflake,
        SnowflakeAuthUtils::determine_auth_type(&credentials).unwrap()
    );
}

#[test]
fn test_create_private_key_with_passphrase() {
    let error =
        SnowflakeAuthUtils::create_private_key("invalid-pem", Some(PASSPHRASE)).unwrap_err();
    assert_invalid_private_key(&error);
}

#[test]
fn test_validate_credentials_with_key_pair_auth() {
    let credentials = credentials(&[
        (snowflake_constants::SF_USER, TESTUSER),
        (snowflake_constants::PEM_PRIVATE_KEY, VALID_PRIVATE_KEY_PEM),
    ]);
    SnowflakeAuthUtils::validate_credentials(&credentials, SnowflakeAuthType::SnowflakeJwt)
        .unwrap();
}

#[test]
fn test_validate_credentials_with_o_auth_auth() {
    let credentials = credentials(&[
        (snowflake_constants::USERNAME, TESTUSER),
        (snowflake_constants::AUTH_CODE, "test-auth-code"),
    ]);
    SnowflakeAuthUtils::validate_credentials(&credentials, SnowflakeAuthType::Oauth).unwrap();
}

#[test]
fn test_validate_credentials_with_password_auth() {
    let credentials = credentials(&[
        (snowflake_constants::USERNAME, TESTUSER),
        (snowflake_constants::PASSWORD, TESTPASSWORD),
    ]);
    SnowflakeAuthUtils::validate_credentials(&credentials, SnowflakeAuthType::Snowflake).unwrap();
}

#[test]
fn test_validate_credentials_with_null_credentials() {
    let error =
        SnowflakeAuthUtils::validate_credentials(&HashMap::new(), SnowflakeAuthType::SnowflakeJwt)
            .unwrap_err();
    assert_illegal_argument(&error, "Credentials cannot be null or empty");
}

#[test]
fn test_validate_credentials_with_key_pair_auth_missing_username() {
    let credentials = credentials(&[(snowflake_constants::PRIVATE_KEY, VALID_PRIVATE_KEY_PEM)]);
    let error =
        SnowflakeAuthUtils::validate_credentials(&credentials, SnowflakeAuthType::SnowflakeJwt)
            .unwrap_err();
    assert_illegal_argument(&error, "Missing required parameter: username/sfUser");
}

#[test]
fn test_validate_credentials_with_key_pair_auth_empty_username() {
    let credentials = credentials(&[
        (snowflake_constants::USERNAME, ""),
        (snowflake_constants::PRIVATE_KEY, VALID_PRIVATE_KEY_PEM),
    ]);
    let error =
        SnowflakeAuthUtils::validate_credentials(&credentials, SnowflakeAuthType::SnowflakeJwt)
            .unwrap_err();
    assert_illegal_argument(&error, "Missing required parameter: username/sfUser");
}

#[test]
fn test_validate_credentials_with_key_pair_auth_missing_private_key() {
    let credentials = credentials(&[(snowflake_constants::USERNAME, TESTUSER)]);
    let error =
        SnowflakeAuthUtils::validate_credentials(&credentials, SnowflakeAuthType::SnowflakeJwt)
            .unwrap_err();
    assert_illegal_argument(
        &error,
        "pem_private_key is required for key-pair authentication",
    );
}

#[test]
fn test_validate_credentials_with_key_pair_auth_empty_private_key() {
    let credentials = credentials(&[
        (snowflake_constants::USERNAME, TESTUSER),
        (snowflake_constants::PRIVATE_KEY, ""),
    ]);
    let error =
        SnowflakeAuthUtils::validate_credentials(&credentials, SnowflakeAuthType::SnowflakeJwt)
            .unwrap_err();
    assert_illegal_argument(
        &error,
        "pem_private_key is required for key-pair authentication",
    );
}

#[test]
fn test_validate_credentials_with_o_auth_auth_missing_username() {
    let credentials = credentials(&[(snowflake_constants::AUTH_CODE, "test-auth-code")]);
    let error = SnowflakeAuthUtils::validate_credentials(&credentials, SnowflakeAuthType::Oauth)
        .unwrap_err();
    assert_illegal_argument(&error, "Missing required parameter: username/sfUser");
}

#[test]
fn test_snowflake_auth_type_from_string() {
    assert_eq!(
        SnowflakeAuthType::Snowflake,
        SnowflakeAuthType::from_string(Some("snowflake")).unwrap()
    );
    assert_eq!(
        SnowflakeAuthType::SnowflakeJwt,
        SnowflakeAuthType::from_string(Some("snowflake_jwt")).unwrap()
    );
    assert_eq!(
        SnowflakeAuthType::Oauth,
        SnowflakeAuthType::from_string(Some("oauth")).unwrap()
    );
}

#[test]
fn test_snowflake_auth_type_get_value() {
    assert_eq!("snowflake", SnowflakeAuthType::Snowflake.value());
    assert_eq!("snowflake_jwt", SnowflakeAuthType::SnowflakeJwt.value());
    assert_eq!("oauth", SnowflakeAuthType::Oauth.value());
}

#[test]
fn test_snowflake_auth_type_to_string() {
    assert_eq!("snowflake", SnowflakeAuthType::Snowflake.to_string());
    assert_eq!("snowflake_jwt", SnowflakeAuthType::SnowflakeJwt.to_string());
    assert_eq!("oauth", SnowflakeAuthType::Oauth.to_string());
}

#[test]
fn test_create_private_key_with_encrypted_key_and_correct_passphrase() {
    let error = SnowflakeAuthUtils::create_private_key(ENCRYPTED_PRIVATE_KEY_PEM, Some(PASSPHRASE))
        .unwrap_err();
    assert_invalid_private_key(&error);

    let key =
        SnowflakeAuthUtils::create_private_key(ENCRYPTED_PRIVATE_KEY_PEM_PBES2, Some(PASSPHRASE))
            .unwrap();
    assert_same_key(&key);
}

#[test]
fn test_create_private_key_with_encrypted_key_and_null_passphrase() {
    let error =
        SnowflakeAuthUtils::create_private_key(ENCRYPTED_PRIVATE_KEY_PEM, None).unwrap_err();
    assert_invalid_private_key(&error);

    let error =
        SnowflakeAuthUtils::create_private_key(ENCRYPTED_PRIVATE_KEY_PEM_PBES2, None).unwrap_err();
    assert_invalid_private_key(&error);
}

#[test]
fn test_create_private_key_with_invalid_encrypted_key() {
    let error =
        SnowflakeAuthUtils::create_private_key(INVALID_ENCRYPTED_PRIVATE_KEY_PEM, Some(PASSPHRASE))
            .unwrap_err();
    assert_invalid_private_key(&error);
}

#[test]
fn test_determine_auth_type_with_encrypted_key_pair_auth() {
    let credentials = credentials(&[
        (snowflake_constants::SF_USER, TESTUSER),
        (
            snowflake_constants::PEM_PRIVATE_KEY,
            ENCRYPTED_PRIVATE_KEY_PEM,
        ),
        (snowflake_constants::PEM_PRIVATE_KEY_PASSPHRASE, PASSPHRASE),
    ]);
    assert_eq!(
        SnowflakeAuthType::SnowflakeJwt,
        SnowflakeAuthUtils::determine_auth_type(&credentials).unwrap()
    );
}

#[test]
fn test_determine_auth_type_with_encrypted_key_pair_auth_missing_passphrase() {
    let credentials = credentials(&[
        (snowflake_constants::SF_USER, TESTUSER),
        (
            snowflake_constants::PEM_PRIVATE_KEY,
            ENCRYPTED_PRIVATE_KEY_PEM,
        ),
    ]);
    assert_eq!(
        SnowflakeAuthType::SnowflakeJwt,
        SnowflakeAuthUtils::determine_auth_type(&credentials).unwrap()
    );
}

#[test]
fn test_validate_credentials_with_encrypted_key_pair_auth() {
    let credentials = credentials(&[
        (snowflake_constants::SF_USER, TESTUSER),
        (
            snowflake_constants::PEM_PRIVATE_KEY,
            ENCRYPTED_PRIVATE_KEY_PEM,
        ),
        (snowflake_constants::PEM_PRIVATE_KEY_PASSPHRASE, PASSPHRASE),
    ]);
    SnowflakeAuthUtils::validate_credentials(&credentials, SnowflakeAuthType::SnowflakeJwt)
        .unwrap();
}

#[test]
fn test_get_username_with_uppercase_key() {
    let credentials = credentials(&[
        (snowflake_constants::USERNAME_UPPERCASE, TESTUSER),
        (snowflake_constants::PASSWORD, TESTPASSWORD),
    ]);
    assert_eq!(
        TESTUSER,
        SnowflakeAuthUtils::username(&credentials).unwrap()
    );
}

#[test]
fn test_get_username_fallback_order() {
    let credentials = credentials(&[
        (snowflake_constants::SF_USER, "sfuser"),
        (snowflake_constants::USERNAME, "username"),
        (snowflake_constants::USERNAME_UPPERCASE, "USERNAME"),
    ]);
    assert_eq!(
        "sfuser",
        SnowflakeAuthUtils::username(&credentials).unwrap()
    );
}

#[test]
fn test_get_username_fallback_to_uppercase() {
    let credentials = credentials(&[
        (snowflake_constants::SF_USER, ""),
        (snowflake_constants::USERNAME, ""),
        (snowflake_constants::USERNAME_UPPERCASE, TESTUSER),
    ]);
    assert_eq!(
        TESTUSER,
        SnowflakeAuthUtils::username(&credentials).unwrap()
    );
}

#[test]
fn test_validate_credentials_with_password_uppercase() {
    let credentials = credentials(&[
        (snowflake_constants::USERNAME, TESTUSER),
        (snowflake_constants::PASSWORD_UPPERCASE, TESTPASSWORD),
    ]);
    SnowflakeAuthUtils::validate_credentials(&credentials, SnowflakeAuthType::Snowflake).unwrap();
}

#[test]
fn test_validate_credentials_with_password_missing_both_cases() {
    let credentials = credentials(&[(snowflake_constants::USERNAME, TESTUSER)]);
    let error =
        SnowflakeAuthUtils::validate_credentials(&credentials, SnowflakeAuthType::Snowflake)
            .unwrap_err();
    assert_illegal_argument(&error, "password is required for password authentication");
}

#[test]
fn test_get_password_falls_back_to_uppercase_when_lowercase_is_blank() {
    let credentials = credentials(&[
        (snowflake_constants::USERNAME, TESTUSER),
        (snowflake_constants::PASSWORD, ""),
        (snowflake_constants::PASSWORD_UPPERCASE, TESTPASSWORD),
    ]);
    SnowflakeAuthUtils::validate_credentials(&credentials, SnowflakeAuthType::Snowflake).unwrap();
    assert_eq!(
        TESTPASSWORD,
        SnowflakeAuthUtils::password(&credentials).unwrap()
    );
}

#[test]
fn test_get_password_prefers_lowercase_key() {
    let credentials = credentials(&[
        (snowflake_constants::PASSWORD, TESTPASSWORD),
        (snowflake_constants::PASSWORD_UPPERCASE, "other-password"),
    ]);
    assert_eq!(
        TESTPASSWORD,
        SnowflakeAuthUtils::password(&credentials).unwrap()
    );
}

#[test]
fn test_create_private_key_from_multi_line_pem() {
    assert_same_key(&SnowflakeAuthUtils::create_private_key(&multi_line_pem(), None).unwrap());
}

#[test]
fn test_create_private_key_from_single_line_pem() {
    let pem = format!(
        "-----BEGIN PRIVATE KEY-----{}-----END PRIVATE KEY-----",
        base64_key()
    );
    assert_same_key(&SnowflakeAuthUtils::create_private_key(&pem, None).unwrap());
}

#[test]
fn test_create_private_key_from_raw_base64() {
    assert_same_key(&SnowflakeAuthUtils::create_private_key(&base64_key(), None).unwrap());
}

#[test]
fn test_create_private_key_from_unencrypted_pem_with_passphrase() {
    assert_same_key(
        &SnowflakeAuthUtils::create_private_key(&multi_line_pem(), Some(PASSPHRASE)).unwrap(),
    );
}

const PBE_PLAIN_KEY: &str = include_str!("fixtures/pbe/plain.p8");
const PBE_PASSPHRASE: &str = "test";
const PBE_KEYS_ACCEPTED_BY_BOUNCY_CASTLE: [(&str, &str); 11] = [
    (
        "pbeWithMD2AndDES-CBC",
        include_str!("fixtures/pbe/pbe_md2_des.p8"),
    ),
    (
        "pbeWithMD5AndDES-CBC",
        include_str!("fixtures/pbe/pbe_md5_des.p8"),
    ),
    (
        "pbeWithMD5AndRC2-CBC",
        include_str!("fixtures/pbe/pbe_md5_rc2_64.p8"),
    ),
    (
        "pbeWithSHA1AndDES-CBC",
        include_str!("fixtures/pbe/pbe_sha1_des.p8"),
    ),
    (
        "pbeWithSHA1AndRC2-CBC",
        include_str!("fixtures/pbe/pbe_sha1_rc2_64.p8"),
    ),
    (
        "pbeWithSHAAnd128BitRC4",
        include_str!("fixtures/pbe/pbe_sha1_rc4_128.p8"),
    ),
    (
        "pbeWithSHAAnd40BitRC4",
        include_str!("fixtures/pbe/pbe_sha1_rc4_40.p8"),
    ),
    (
        "pbeWithSHAAnd3-KeyTripleDES-CBC",
        include_str!("fixtures/pbe/pbe_sha1_3des.p8"),
    ),
    (
        "pbeWithSHAAnd2-KeyTripleDES-CBC",
        include_str!("fixtures/pbe/pbe_sha1_2des.p8"),
    ),
    (
        "pbeWithSHAAnd128BitRC2-CBC",
        include_str!("fixtures/pbe/pbe_sha1_rc2_128.p8"),
    ),
    (
        "pbeWithSHAAnd40BitRC2-CBC",
        include_str!("fixtures/pbe/pbe_sha1_rc2_40.p8"),
    ),
];
const PBE_MD2_RC2_KEY: &str = include_str!("fixtures/pbe/pbe_md2_rc2_64.p8");

fn pbe_plain_der() -> Vec<u8> {
    SnowflakeAuthUtils::create_private_key(PBE_PLAIN_KEY, None)
        .unwrap()
        .der()
        .to_vec()
}

const JAVA_ENCODED_SHA256: &str =
    "62e7c614258732ba42618d3f1cf14b46278eff50cbb703df906bec522db118aa";
const JAVA_ENCODED_WITHOUT_NULL_SHA256: &str =
    "c13490a11c777d3298a0da21a838d530043b0e2aeb00a34c5a9573fa2ec818b5";

const BER_PRIVATE_KEY_INFOS: [(&str, &str); 20] = [
    (
        "enc_algid_without_null",
        include_str!("fixtures/inner_ber/enc_algid_without_null.p8"),
    ),
    (
        "enc_all_indefinite",
        include_str!("fixtures/inner_ber/enc_all_indefinite.p8"),
    ),
    (
        "enc_constructed_octet_string",
        include_str!("fixtures/inner_ber/enc_constructed_octet_string.p8"),
    ),
    ("enc_der", include_str!("fixtures/inner_ber/enc_der.p8")),
    (
        "enc_inner_and_outer_ber",
        include_str!("fixtures/inner_ber/enc_inner_and_outer_ber.p8"),
    ),
    (
        "enc_inner_rsa_indefinite",
        include_str!("fixtures/inner_ber/enc_inner_rsa_indefinite.p8"),
    ),
    (
        "enc_inner_rsa_long_form",
        include_str!("fixtures/inner_ber/enc_inner_rsa_long_form.p8"),
    ),
    (
        "enc_long_form_lengths",
        include_str!("fixtures/inner_ber/enc_long_form_lengths.p8"),
    ),
    (
        "enc_outer_indefinite",
        include_str!("fixtures/inner_ber/enc_outer_indefinite.p8"),
    ),
    (
        "enc_with_attributes",
        include_str!("fixtures/inner_ber/enc_with_attributes.p8"),
    ),
    (
        "plain_algid_without_null",
        include_str!("fixtures/inner_ber/plain_algid_without_null.p8"),
    ),
    (
        "plain_all_indefinite",
        include_str!("fixtures/inner_ber/plain_all_indefinite.p8"),
    ),
    (
        "plain_constructed_octet_string",
        include_str!("fixtures/inner_ber/plain_constructed_octet_string.p8"),
    ),
    ("plain_der", include_str!("fixtures/inner_ber/plain_der.p8")),
    (
        "plain_inner_and_outer_ber",
        include_str!("fixtures/inner_ber/plain_inner_and_outer_ber.p8"),
    ),
    (
        "plain_inner_rsa_indefinite",
        include_str!("fixtures/inner_ber/plain_inner_rsa_indefinite.p8"),
    ),
    (
        "plain_inner_rsa_long_form",
        include_str!("fixtures/inner_ber/plain_inner_rsa_long_form.p8"),
    ),
    (
        "plain_long_form_lengths",
        include_str!("fixtures/inner_ber/plain_long_form_lengths.p8"),
    ),
    (
        "plain_outer_indefinite",
        include_str!("fixtures/inner_ber/plain_outer_indefinite.p8"),
    ),
    (
        "plain_with_attributes",
        include_str!("fixtures/inner_ber/plain_with_attributes.p8"),
    ),
];

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .concat()
}

#[test]
fn test_create_private_key_reencodes_ber_private_key_info_like_get_encoded() {
    assert_eq!(JAVA_ENCODED_SHA256, sha256_hex(&pbe_plain_der()));
    for (name, pem) in BER_PRIVATE_KEY_INFOS {
        let passphrase = name.starts_with("enc_").then_some(PBE_PASSPHRASE);
        let key = SnowflakeAuthUtils::create_private_key(pem, passphrase)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let expected = if name.ends_with("algid_without_null") {
            JAVA_ENCODED_WITHOUT_NULL_SHA256
        } else {
            JAVA_ENCODED_SHA256
        };
        assert_eq!(expected, sha256_hex(key.der()), "{name}");
    }
}

#[test]
fn test_create_private_key_with_pbes1_and_pkcs12_encrypted_keys() {
    let expected = pbe_plain_der();
    for (scheme, pem) in PBE_KEYS_ACCEPTED_BY_BOUNCY_CASTLE {
        let key = SnowflakeAuthUtils::create_private_key(pem, Some(PBE_PASSPHRASE))
            .unwrap_or_else(|e| panic!("{scheme}: {e}"));
        assert_eq!(expected, key.der(), "{scheme}");
    }
}

#[test]
fn test_create_private_key_with_pbes1_and_pkcs12_encrypted_keys_and_wrong_passphrase() {
    for (scheme, pem) in PBE_KEYS_ACCEPTED_BY_BOUNCY_CASTLE {
        let error = SnowflakeAuthUtils::create_private_key(pem, Some("wrong")).unwrap_err();
        assert_eq!("Invalid private key format", error.to_string(), "{scheme}");
    }
}

#[test]
fn test_create_private_key_with_pbe_md2_and_rc2_is_rejected() {
    let error =
        SnowflakeAuthUtils::create_private_key(PBE_MD2_RC2_KEY, Some(PBE_PASSPHRASE)).unwrap_err();
    assert_invalid_private_key(&error);
}

const PBE_KEYS_OVER_THE_BOUNCY_CASTLE_ITERATION_LIMIT: [(&str, &str); 4] = [
    (
        "pbeWithMD5AndDES-CBC 10000001",
        include_str!("fixtures/pbe/pbe_md5_des_iter_10000001.p8"),
    ),
    (
        "pbeWithMD5AndDES-CBC 2147483647",
        include_str!("fixtures/pbe/pbe_md5_des_iter_2147483647.p8"),
    ),
    (
        "pbeWithMD5AndDES-CBC 4294967295",
        include_str!("fixtures/pbe/pbe_md5_des_iter_4294967295.p8"),
    ),
    (
        "pbeWithSHAAnd3-KeyTripleDES-CBC 2147483647",
        include_str!("fixtures/pbe/pbe_sha1_3des_iter_2147483647.p8"),
    ),
];

#[test]
fn test_create_private_key_rejects_pbe_iteration_count_over_bouncy_castle_limit() {
    let started = std::time::Instant::now();
    for (scheme, pem) in PBE_KEYS_OVER_THE_BOUNCY_CASTLE_ITERATION_LIMIT {
        let error = SnowflakeAuthUtils::create_private_key(pem, Some(PBE_PASSPHRASE)).unwrap_err();
        assert_invalid_private_key(&error);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "{scheme} took {:?}",
            started.elapsed()
        );
    }
}

const PBES2_KEYS_ACCEPTED_BY_BOUNCY_CASTLE: [(&str, &str); 31] = [
    (
        "pbkdf2 aes256 key length 0",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_aes256_key_length_0.p8"),
    ),
    (
        "pbkdf2 aes256 key length 1099511627776",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_aes256_key_length_1099511627776.p8"),
    ),
    (
        "pbkdf2 aes256 key length 16",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_aes256_key_length_16.p8"),
    ),
    (
        "pbkdf2 aes256 key length 64",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_aes256_key_length_64.p8"),
    ),
    (
        "pbkdf2 aes256 key length minus 1",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_aes256_key_length_minus_1.p8"),
    ),
    (
        "pbkdf2 ber all indefinite",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_ber_all_indefinite.p8"),
    ),
    (
        "pbkdf2 ber constructed ciphertext",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_ber_constructed_ciphertext.p8"),
    ),
    (
        "pbkdf2 ber long form lengths",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_ber_long_form_lengths.p8"),
    ),
    (
        "pbkdf2 ber outer indefinite",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_ber_outer_indefinite.p8"),
    ),
    (
        "pbkdf2 extra trailing element",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_extra_trailing_element.p8"),
    ),
    (
        "pbkdf2 gost",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_gost.p8"),
    ),
    (
        "pbkdf2 sha1 without null",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_sha1_without_null.p8"),
    ),
    (
        "pbkdf2 sha224",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_sha224.p8"),
    ),
    (
        "pbkdf2 sha256 without null",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_sha256_without_null.p8"),
    ),
    (
        "pbkdf2 sha3 224",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_sha3_224.p8"),
    ),
    (
        "pbkdf2 sha3 256",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_sha3_256.p8"),
    ),
    (
        "pbkdf2 sha3 384",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_sha3_384.p8"),
    ),
    (
        "pbkdf2 sha3 512",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_sha3_512.p8"),
    ),
    (
        "pbkdf2 sha384",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_sha384.p8"),
    ),
    (
        "pbkdf2 sha512",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_sha512.p8"),
    ),
    (
        "pbkdf2 sm3",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_sm3.p8"),
    ),
    (
        "pbkdf2 sm4",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_sm4.p8"),
    ),
    (
        "scrypt ber outer indefinite",
        include_str!("fixtures/pbes2/pbes2_scrypt_ber_outer_indefinite.p8"),
    ),
    (
        "PBKDF2 2048",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_aes256_iter_2048.p8"),
    ),
    (
        "scrypt N=1024 r=8 p=1",
        include_str!("fixtures/pbes2/pbes2_scrypt_aes256_n_1024_r_8_p_1.p8"),
    ),
    (
        "scrypt N=2 r=1 p=65536",
        include_str!("fixtures/pbes2/pbes2_scrypt_aes128_n_2_r_1_p_65536.p8"),
    ),
    (
        "scrypt DES-EDE3",
        include_str!("fixtures/pbes2/pbes2_scrypt_3des_n_2_r_1_p_1.p8"),
    ),
    (
        "scrypt keyLength ignored",
        include_str!("fixtures/pbes2/pbes2_scrypt_aes256_key_length_16.p8"),
    ),
    (
        "scrypt p=4294967297 read as int 1",
        include_str!("fixtures/pbes2/pbes2_scrypt_aes128_p_4294967297.p8"),
    ),
    (
        "scrypt N=2 r=1024 p=2",
        include_str!("fixtures/pbes2/pbes2_scrypt_aes256_n_2_r_1024_p_2.p8"),
    ),
    (
        "scrypt N=4 r=512",
        include_str!("fixtures/pbes2/pbes2_scrypt_aes256_n_4_r_512.p8"),
    ),
];

const PBES2_KEYS_REJECTED_BY_BOUNCY_CASTLE: [(&str, &str); 24] = [
    (
        "pbkdf2 ber data after end of contents",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_ber_data_after_end_of_contents.p8"),
    ),
    (
        "pbkdf2 ber missing end of contents",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_ber_missing_end_of_contents.p8"),
    ),
    (
        "pbkdf2 md5",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_md5.p8"),
    ),
    (
        "pbkdf2 sha512 256",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_sha512_256.p8"),
    ),
    (
        "scrypt aes256 n 3",
        include_str!("fixtures/pbes2/pbes2_scrypt_aes256_n_3.p8"),
    ),
    (
        "scrypt aes256 n 65536 r 1",
        include_str!("fixtures/pbes2/pbes2_scrypt_aes256_n_65536_r_1.p8"),
    ),
    (
        "scrypt aes256 n 6",
        include_str!("fixtures/pbes2/pbes2_scrypt_aes256_n_6.p8"),
    ),
    (
        "scrypt aes256 r 1024 p 2048",
        include_str!("fixtures/pbes2/pbes2_scrypt_aes256_r_1024_p_2048.p8"),
    ),
    (
        "PBKDF2 10000001",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_aes256_iter_10000001.p8"),
    ),
    (
        "PBKDF2 2147483647",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_aes256_iter_2147483647.p8"),
    ),
    (
        "PBKDF2 2147483648",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_aes256_iter_2147483648.p8"),
    ),
    (
        "PBKDF2 -1",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_aes256_iter_minus_1.p8"),
    ),
    (
        "PBKDF2 0",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_aes256_iter_0.p8"),
    ),
    (
        "PBKDF2 empty salt",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_aes256_empty_salt.p8"),
    ),
    (
        "scrypt N=0",
        include_str!("fixtures/pbes2/pbes2_scrypt_aes256_n_0.p8"),
    ),
    (
        "scrypt N=1",
        include_str!("fixtures/pbes2/pbes2_scrypt_aes256_n_1.p8"),
    ),
    (
        "scrypt N=2147483648",
        include_str!("fixtures/pbes2/pbes2_scrypt_aes256_n_2147483648.p8"),
    ),
    (
        "scrypt N=4 r=513",
        include_str!("fixtures/pbes2/pbes2_scrypt_aes256_n_4_r_513.p8"),
    ),
    (
        "scrypt r=1025",
        include_str!("fixtures/pbes2/pbes2_scrypt_aes256_r_1025.p8"),
    ),
    (
        "scrypt r=256 p=8192",
        include_str!("fixtures/pbes2/pbes2_scrypt_aes256_r_256_p_8192.p8"),
    ),
    (
        "scrypt r=8 p=262144",
        include_str!("fixtures/pbes2/pbes2_scrypt_aes256_r_8_p_262144.p8"),
    ),
    (
        "scrypt N=8192 r=1024",
        include_str!("fixtures/pbes2/pbes2_scrypt_aes256_n_8192_r_1024.p8"),
    ),
    (
        "scrypt N=16384 r=1024",
        include_str!("fixtures/pbes2/pbes2_scrypt_aes256_n_16384_r_1024.p8"),
    ),
    (
        "scrypt N=1073741824 r=8",
        include_str!("fixtures/pbes2/pbes2_scrypt_aes256_n_1073741824_r_8.p8"),
    ),
];

const PBES2_UNICODE_PASSPHRASE: &str = "pässwörd€😀";

const PBES2_UNICODE_KEYS_ACCEPTED_BY_BOUNCY_CASTLE: [(&str, &str); 4] = [
    (
        "pbkdf2 prf absent unicode 8bit",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_prf_absent_unicode_8bit.p8"),
    ),
    (
        "pbkdf2 sha1 unicode 8bit",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_sha1_unicode_8bit.p8"),
    ),
    (
        "pbkdf2 sha256 unicode utf8",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_sha256_unicode_utf8.p8"),
    ),
    (
        "pbkdf2 sha3 256 unicode utf8",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_sha3_256_unicode_utf8.p8"),
    ),
];

const PBES2_UNICODE_KEYS_REJECTED_BY_BOUNCY_CASTLE: [(&str, &str); 3] = [
    (
        "pbkdf2 prf absent unicode utf8",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_prf_absent_unicode_utf8.p8"),
    ),
    (
        "pbkdf2 sha1 unicode utf8",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_sha1_unicode_utf8.p8"),
    ),
    (
        "pbkdf2 sha256 unicode 8bit",
        include_str!("fixtures/pbes2/pbes2_pbkdf2_sha256_unicode_8bit.p8"),
    ),
];

#[test]
fn test_create_private_key_encodes_pbes2_unicode_passphrase_like_bouncy_castle() {
    let expected = pbe_plain_der();
    for (scheme, pem) in PBES2_UNICODE_KEYS_ACCEPTED_BY_BOUNCY_CASTLE {
        let key = SnowflakeAuthUtils::create_private_key(pem, Some(PBES2_UNICODE_PASSPHRASE))
            .unwrap_or_else(|e| panic!("{scheme}: {e}"));
        assert_eq!(expected, key.der(), "{scheme}");
    }
    for (scheme, pem) in PBES2_UNICODE_KEYS_REJECTED_BY_BOUNCY_CASTLE {
        let result = SnowflakeAuthUtils::create_private_key(pem, Some(PBES2_UNICODE_PASSPHRASE));
        assert_invalid_private_key(&result.expect_err(scheme));
    }
}

#[test]
fn test_create_private_key_with_pbes2_keys_accepted_by_bouncy_castle() {
    let expected = pbe_plain_der();
    for (scheme, pem) in PBES2_KEYS_ACCEPTED_BY_BOUNCY_CASTLE {
        let key = SnowflakeAuthUtils::create_private_key(pem, Some(PBE_PASSPHRASE))
            .unwrap_or_else(|e| panic!("{scheme}: {e}"));
        assert_eq!(expected, key.der(), "{scheme}");
    }
}

#[test]
fn test_create_private_key_rejects_pbes2_keys_over_bouncy_castle_limits() {
    let started = std::time::Instant::now();
    for (scheme, pem) in PBES2_KEYS_REJECTED_BY_BOUNCY_CASTLE {
        let result = SnowflakeAuthUtils::create_private_key(pem, Some(PBE_PASSPHRASE));
        assert_invalid_private_key(&result.expect_err(scheme));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "{scheme} took {:?}",
            started.elapsed()
        );
    }
}
