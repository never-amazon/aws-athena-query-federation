/*-
 * #%L
 * athena-snowflake
 * %%
 * Copyright (C) 2019 - 2025 Amazon Web Services
 * %%
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 * #L%
 */
package com.amazonaws.athena.connectors.snowflake.connection;

import com.amazonaws.athena.connector.credentials.CredentialsProvider;
import com.amazonaws.athena.connectors.jdbc.connection.DatabaseConnectionConfig;
import com.amazonaws.athena.connectors.jdbc.connection.DatabaseConnectionInfo;
import com.amazonaws.athena.connectors.jdbc.connection.GenericJdbcConnectionFactory;
import com.amazonaws.athena.connectors.snowflake.SnowflakeConstants;
import com.amazonaws.athena.connectors.snowflake.utils.SnowflakeAuthUtils;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.net.URLDecoder;
import java.nio.charset.StandardCharsets;
import java.sql.Connection;
import java.sql.DriverManager;
import java.sql.SQLException;
import java.util.Map;
import java.util.Properties;
import java.util.regex.Matcher;

import static com.amazonaws.athena.connectors.snowflake.SnowflakeConstants.AUTHENTICATOR;
import static com.amazonaws.athena.connectors.snowflake.SnowflakeConstants.PRIVATE_KEY;
import static com.amazonaws.athena.connectors.snowflake.SnowflakeConstants.USER;
import static com.amazonaws.athena.connectors.snowflake.utils.SnowflakeAuthType.SNOWFLAKE_JWT;

/**
 * Custom connection factory for Snowflake that supports multiple authentication methods.
 */
public class SnowflakeConnectionFactory extends GenericJdbcConnectionFactory
{
    private static final Logger LOGGER = LoggerFactory.getLogger(SnowflakeConnectionFactory.class);

    // Constructor properties only. The parent's jdbcProperties also collects every pooled caller's credentials.
    private final Properties keyPairBaseProperties = new Properties();

    public SnowflakeConnectionFactory(DatabaseConnectionConfig databaseConnectionConfig, Map<String, String> properties, DatabaseConnectionInfo databaseConnectionInfo)
    {
        this(databaseConnectionConfig, properties, databaseConnectionInfo, null);
    }

    public SnowflakeConnectionFactory(DatabaseConnectionConfig databaseConnectionConfig, Map<String, String> properties, DatabaseConnectionInfo databaseConnectionInfo, final Map<String, String> configOptions)
    {
        super(databaseConnectionConfig, properties, databaseConnectionInfo, configOptions);
        if (properties != null) {
            keyPairBaseProperties.putAll(properties);
        }
    }

    @Override
    public Connection getConnection(CredentialsProvider credentialsProvider) throws Exception
    {
        if (credentialsProvider == null) {
            return super.getConnectionFromManagedPool(null);
        }

        Map<String, String> credentialMap = credentialsProvider.getCredentialMap();
        if (SNOWFLAKE_JWT.equals(SnowflakeAuthUtils.determineAuthType(credentialMap))) {
            // HikariCP stringifies property values, which breaks the PrivateKey object, so key-pair auth uses DriverManager.
            return getKeyPairConnection(credentialMap);
        }
        // Password and OAuth always use the pool, even when fas_token is set.
        return super.getConnectionFromManagedPool(credentialsProvider);
    }

    /**
     * Creates a connection using key-pair authentication via {@link DriverManager}.
     */
    private Connection getKeyPairConnection(Map<String, String> credentialMap) throws SQLException
    {
        try {
            String username = credentialMap.get(USER);
            String privateKeyPem = credentialMap.get(SnowflakeConstants.PEM_PRIVATE_KEY);
            String passphrase = credentialMap.get(SnowflakeConstants.PEM_PRIVATE_KEY_PASSPHRASE);

            java.security.PrivateKey privateKey = SnowflakeAuthUtils.createPrivateKey(privateKeyPem, passphrase);

            Properties props = new Properties();
            props.putAll(keyPairBaseProperties);
            String baseUrl = moveQueryParametersToProperties(buildConnectionString(), props);
            props.setProperty(USER, username);
            props.setProperty(AUTHENTICATOR, SNOWFLAKE_JWT.getValue());
            props.put(PRIVATE_KEY, privateKey);

            LOGGER.debug("Creating Snowflake connection with key-pair authentication for user: {}", username);

            // DriverManager may not have the Snowflake driver registered on this path, so load it explicitly.
            Class.forName(databaseConnectionInfo.getDriverClassName());

            return DriverManager.getConnection(baseUrl, props);
        }
        catch (Exception e) {
            LOGGER.error("Failed to create Snowflake connection with key-pair authentication", e);
            throw new SQLException("Failed to create Snowflake connection with key-pair authentication: " + e.getMessage(), e);
        }
    }

    /**
     * Builds the JDBC connection string for Snowflake.
     * 
     * @return The connection string
     */
    private String buildConnectionString()
    {
        String jdbcString = databaseConnectionConfig.getJdbcConnectionString();
        
        // Remove any secret placeholders from the connection string
        Matcher secretMatcher = SECRET_NAME_PATTERN.matcher(jdbcString);
        return secretMatcher.replaceAll(Matcher.quoteReplacement(""));
    }

    /**
     * Moves the URL's query parameters into {@code props}, URL-decoded, and returns the URL without them.
     * The driver's {@code SnowflakeConnectString.parse()} rejects a URL that contains decoded double quotes.
     */
    private static String moveQueryParametersToProperties(String url, Properties props)
    {
        int queryIndex = url.indexOf('?');
        if (queryIndex < 0) {
            return url;
        }
        for (String param : url.substring(queryIndex + 1).split("&")) {
            String[] kv = param.split("=", 2);
            if (kv.length == 2) {
                props.setProperty(
                        URLDecoder.decode(kv[0], StandardCharsets.UTF_8),
                        URLDecoder.decode(kv[1], StandardCharsets.UTF_8));
            }
        }
        return url.substring(0, queryIndex);
    }
}
