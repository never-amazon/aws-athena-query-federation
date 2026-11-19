use std::collections::HashMap;
use std::path::Path;

use athena_snowflake::snowflake_constants;
use athena_snowflake::sql::parse_connection_settings;
use athena_snowflake::utils::SnowflakeAuthUtils;
use snowflake_connector_rs::Client;

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

#[test]
fn test_install_ca_certificate() {
    assert_eq!("SSL_CERT_FILE", snowflake_constants::SSL_CERT_FILE_LOCATION);
    assert_eq!(
        "/tmp/cacert.pem",
        snowflake_constants::SSL_CERT_FILE_LOCATION_VALUE
    );

    let certificate_file = Path::new(snowflake_constants::SSL_CERT_FILE_LOCATION_VALUE);
    let existed_before = certificate_file.exists();

    let private_key =
        SnowflakeAuthUtils::create_private_key(UNENCRYPTED_PRIVATE_KEY_PEM, None).unwrap();
    let properties = HashMap::from([
        ("user".to_string(), "testuser".to_string()),
        ("authenticator".to_string(), "snowflake_jwt".to_string()),
    ]);
    let settings =
        parse_connection_settings("jdbc:snowflake://test.snowflakecomputing.com", &properties)
            .unwrap();
    assert_eq!("https", settings.base_url.scheme());
    let config = settings.client_config(Some(&private_key)).unwrap();
    Client::new(config).unwrap();

    assert_eq!(existed_before, certificate_file.exists());
}
