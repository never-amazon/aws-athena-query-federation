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
package com.amazonaws.athena.connectors.snowflake.utils;

import com.amazonaws.athena.connector.lambda.exceptions.AthenaConnectorException;
import com.amazonaws.athena.connectors.snowflake.SnowflakeConstants;
import org.apache.commons.lang3.StringUtils;
import org.bouncycastle.asn1.pkcs.PrivateKeyInfo;
import org.bouncycastle.jce.provider.BouncyCastleProvider;
import org.bouncycastle.openssl.PEMParser;
import org.bouncycastle.openssl.jcajce.JcaPEMKeyConverter;
import org.bouncycastle.openssl.jcajce.JceOpenSSLPKCS8DecryptorProviderBuilder;
import org.bouncycastle.operator.InputDecryptorProvider;
import org.bouncycastle.pkcs.PKCS8EncryptedPrivateKeyInfo;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import software.amazon.awssdk.services.glue.model.ErrorDetails;
import software.amazon.awssdk.services.glue.model.FederationSourceErrorCode;

import java.io.StringReader;
import java.security.PrivateKey;
import java.security.Security;
import java.util.Map;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * Utility class for Snowflake authentication methods.
 */
public class SnowflakeAuthUtils
{
    private static final Logger LOGGER = LoggerFactory.getLogger(SnowflakeAuthUtils.class);
    private static final Pattern PEM_BEGIN_LABEL = Pattern.compile("-----BEGIN ([^-]+)-----");
    private static final int PEM_LINE_LENGTH = 64;

    private SnowflakeAuthUtils()
    {
    }

    /**
     * Determines the authentication type based on the provided credentials.
     * 
     * @param credentials The credentials map from AWS Secrets Manager
     * @return The authentication type enum
     */
    public static SnowflakeAuthType determineAuthType(Map<String, String> credentials)
    {
        if (credentials == null || credentials.isEmpty()) {
            throw new IllegalArgumentException("Credentials cannot be null or empty");
        }

        SnowflakeAuthType authType;
        if (StringUtils.isNotBlank(credentials.get(SnowflakeConstants.PEM_PRIVATE_KEY))) {
            LOGGER.debug("Key-pair authentication detected");
            authType = SnowflakeAuthType.SNOWFLAKE_JWT;
        }
        else if (StringUtils.isNotBlank(credentials.get(SnowflakeConstants.AUTH_CODE))) {
            LOGGER.debug("OAuth authentication detected");
            authType = SnowflakeAuthType.OAUTH;
        }
        else {
            LOGGER.debug("Password authentication detected");
            authType = SnowflakeAuthType.SNOWFLAKE;
        }
        return authType;
    }

    /**
     * Creates a PrivateKey object from a private key string.
     * Supports both PEM-formatted keys (with headers/footers) and raw base64-encoded keys (without headers/footers).
     * Supports both encrypted and unencrypted private keys using Java standard libraries.
     * 
     * Supported PEM header types:
     * - -----BEGIN PRIVATE KEY----- (PKCS8 unencrypted)
     * - -----BEGIN ENCRYPTED PRIVATE KEY----- (PKCS8 encrypted)
     * 
     * @param privateKeyString The private key string (PEM-formatted or raw base64)
     * @param passphrase The passphrase for encrypted private keys (can be null for unencrypted keys)
     * @return PrivateKey object
     * @throws Exception if the private key cannot be parsed
     */
    public static PrivateKey createPrivateKey(String privateKeyString, String passphrase) throws Exception
    {
        try {
            PrivateKeyInfo privateKeyInfo = null;
            Security.addProvider(new BouncyCastleProvider());
            
            // Rewrite the private key string as a well-formed PEM
            String pemFormattedKey = normalizeToPem(privateKeyString, passphrase);
            
            // Parse the PEM object and create PrivateKey
            privateKeyInfo = parsePemObject(pemFormattedKey, passphrase);
            
            JcaPEMKeyConverter converter = new JcaPEMKeyConverter().setProvider(BouncyCastleProvider.PROVIDER_NAME);
            return converter.getPrivateKey(privateKeyInfo);
        }
        catch (Exception e) {
            LOGGER.error("Private key parsing failed: {}", e.getMessage());
            throw new AthenaConnectorException("Invalid private key format", ErrorDetails.builder().errorCode(FederationSourceErrorCode.INVALID_INPUT_EXCEPTION.toString()).build());
        }
    }

    /**
     * Rewrites a private key string as a PEM with 64-character lines.
     *
     * @param privateKeyString The private key string (PEM-formatted or raw base64)
     * @param passphrase The passphrase for encrypted private keys (can be null for unencrypted keys)
     * @return Properly formatted PEM string
     */
    private static String normalizeToPem(String privateKeyString, String passphrase)
    {
        // PEMParser rejects a PEM on a single line, which is how JSON secrets often store it, so re-wrap the body.
        // A BEGIN header keeps its own label. Only header-less base64 takes its label from the passphrase.
        Matcher beginLabel = PEM_BEGIN_LABEL.matcher(privateKeyString);
        final String keyType;
        if (beginLabel.find()) {
            keyType = beginLabel.group(1).trim();
        }
        else if (passphrase != null && !passphrase.trim().isEmpty()) {
            keyType = "ENCRYPTED PRIVATE KEY";
        }
        else {
            keyType = "PRIVATE KEY";
        }

        String body = privateKeyString
                .replaceAll("-----BEGIN[^-]*-----", "")
                .replaceAll("-----END[^-]*-----", "")
                .replaceAll("\\s", "");

        StringBuilder pem = new StringBuilder();
        pem.append("-----BEGIN ").append(keyType).append("-----\n");
        for (int i = 0; i < body.length(); i += PEM_LINE_LENGTH) {
            pem.append(body, i, Math.min(i + PEM_LINE_LENGTH, body.length())).append('\n');
        }
        pem.append("-----END ").append(keyType).append("-----\n");
        LOGGER.debug("Normalized private key into well-formed PEM ({})", keyType);
        return pem.toString();
    }

    /**
     * Parses a PEM-formatted private key string and returns the PrivateKeyInfo.
     * 
     * @param pemFormattedKey The PEM-formatted private key string
     * @param passphrase The passphrase for encrypted private keys (can be null for unencrypted keys)
     * @return PrivateKeyInfo object
     * @throws Exception if the private key cannot be parsed
     */
    private static PrivateKeyInfo parsePemObject(String pemFormattedKey, String passphrase) throws Exception
    {
        PEMParser pemParser = new PEMParser(new StringReader(pemFormattedKey));
        try {
            Object pemObject = pemParser.readObject();
            if (pemObject instanceof PKCS8EncryptedPrivateKeyInfo) {
                // Handle the case where the private key is encrypted.
                PKCS8EncryptedPrivateKeyInfo encryptedPrivateKeyInfo = (PKCS8EncryptedPrivateKeyInfo) pemObject;
                if (passphrase == null || passphrase.trim().isEmpty()) {
                    throw new Exception("Passphrase is required for encrypted private key");
                }
                InputDecryptorProvider pkcs8Prov = new JceOpenSSLPKCS8DecryptorProviderBuilder().build(passphrase.toCharArray());
                return encryptedPrivateKeyInfo.decryptPrivateKeyInfo(pkcs8Prov);
            }
            else if (pemObject instanceof PrivateKeyInfo) {
                // Handle the case where the private key is unencrypted.
                return (PrivateKeyInfo) pemObject;
            }
            else {
                throw new Exception("Unsupported private key format. Expected PKCS8EncryptedPrivateKeyInfo or PrivateKeyInfo, got: " + 
                                  (pemObject != null ? pemObject.getClass().getSimpleName() : "null"));
            }
        }
        finally {
            pemParser.close();
        }
    }

    /**
     * Gets the username from credentials, checking both "username" and "user" fields.
     * 
     * @param credentials The credentials map
     * @return The username value
     * @throws IllegalArgumentException if neither field is present or is empty
     */
    public static String getUsername(Map<String, String> credentials)
    {
        String username = credentials.get(SnowflakeConstants.SF_USER);
        if (StringUtils.isBlank(username)) {
            //for oauth and password auth type
            //this can be removed once changes to sfUser instead of username
            username = credentials.get(SnowflakeConstants.USERNAME);
        }
        if (StringUtils.isBlank(username)) {
            username = credentials.get(SnowflakeConstants.USERNAME_UPPERCASE);
        }
        if (StringUtils.isBlank(username)) {
            throw new IllegalArgumentException("Missing required parameter: username/sfUser");
        }
        return username;
    }

    /**
     * Gets the password from credentials, falling back to "PASSWORD" when "password" is blank.
     *
     * @param credentials The credentials map
     * @return The password value
     * @throws IllegalArgumentException if both fields are missing or blank
     */
    public static String getPassword(Map<String, String> credentials)
    {
        String password = credentials.get(SnowflakeConstants.PASSWORD);
        if (StringUtils.isBlank(password)) {
            password = credentials.get(SnowflakeConstants.PASSWORD_UPPERCASE);
        }
        if (StringUtils.isBlank(password)) {
            throw new IllegalArgumentException("password is required for password authentication");
        }
        return password;
    }

    /**
     * Validates that the credentials contain the required fields for the authentication type.
     * 
     * @param credentials The credentials map
     * @param authType The authentication type
     * @throws IllegalArgumentException if required fields are missing
     */
    public static void validateCredentials(Map<String, String> credentials, SnowflakeAuthType authType)
    {
        if (credentials == null || credentials.isEmpty()) {
            throw new IllegalArgumentException("Credentials cannot be null or empty");
        }
        // Check for sfUser (or "username" / "USERNAME" field)
        getUsername(credentials);

        switch (authType) {
            case SNOWFLAKE_JWT:
                if (StringUtils.isBlank(credentials.get(SnowflakeConstants.PEM_PRIVATE_KEY))) {
                    throw new IllegalArgumentException("pem_private_key is required for key-pair authentication");
                }
                // Note: Passphrase is optional - only required if the private key is encrypted
                break;
            case OAUTH:
                if (StringUtils.isBlank(credentials.get(SnowflakeConstants.AUTH_CODE))) {
                    throw new IllegalArgumentException("Auth code is required for OAuth authentication");
                }
                break;
            case SNOWFLAKE:
                getPassword(credentials);
                break;
            default:
                throw new IllegalArgumentException("Unsupported authentication type: " + authType);
        }
    }
}
