package com.amazonaws.athena.connectors.postgresql;

import com.amazonaws.athena.connector.lambda.handlers.CompositeHandler;
import com.amazonaws.athena.connector.lambda.security.EncryptionKey;
import com.amazonaws.athena.connector.lambda.security.LocalKeyFactory;
import com.amazonaws.athena.connectors.jdbc.connection.DatabaseConnectionConfig;
import com.amazonaws.athena.connectors.jdbc.connection.DatabaseConnectionConfigBuilder;
import com.amazonaws.athena.connectors.jdbc.connection.DatabaseConnectionInfo;
import com.amazonaws.athena.connectors.jdbc.connection.GenericJdbcConnectionFactory;
import com.amazonaws.athena.connectors.jdbc.manager.JDBCUtil;
import com.amazonaws.athena.connectors.jdbc.manager.JdbcMetadataHandler;
import com.amazonaws.athena.connectors.jdbc.manager.JdbcRecordHandler;
import software.amazon.awssdk.awscore.AwsRequestOverrideConfiguration;
import software.amazon.awssdk.services.athena.AthenaClient;
import software.amazon.awssdk.services.s3.S3Client;
import software.amazon.awssdk.services.secretsmanager.SecretsManagerClient;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

import static com.amazonaws.athena.connectors.postgresql.PostGreSqlConstants.POSTGRESQL_DEFAULT_PORT;
import static com.amazonaws.athena.connectors.postgresql.PostGreSqlConstants.POSTGRESQL_DRIVER_CLASS;
import static com.amazonaws.athena.connectors.postgresql.PostGreSqlConstants.POSTGRES_NAME;
import static com.amazonaws.athena.connectors.postgresql.PostGreSqlConstants.POSTGRES_QUOTE_CHARACTER;

public final class ParityHandlers implements AutoCloseable
{
    private final CompositeHandler composite;
    private final List<AutoCloseable> closeables = new ArrayList<>();

    private ParityHandlers(String mode, Map<String, String> config, S3Client s3, SecretsManagerClient secrets, AthenaClient athena)
    {
        if (mode.equals("single")) {
            DatabaseConnectionConfig dbConfig = JDBCUtil.getSingleDatabaseConfigFromEnv(POSTGRES_NAME, config);
            PostGreSqlMetadataHandler metadata = metadataHandler(dbConfig, config, secrets, athena);
            PostGreSqlRecordHandler record = recordHandler(dbConfig, config, s3, secrets, athena);
            closeables.add(metadata);
            closeables.add(record);
            composite = new CompositeHandler(metadata, record);
        }
        else if (mode.equals("mux")) {
            String functionName = config.get("AWS_LAMBDA_FUNCTION_NAME");
            List<DatabaseConnectionConfig> configs = new DatabaseConnectionConfigBuilder().engine(POSTGRES_NAME).properties(config).build();
            Map<String, JdbcMetadataHandler> metadataMap = new HashMap<>();
            Map<String, JdbcRecordHandler> recordMap = new HashMap<>();
            DatabaseConnectionConfig defaultConfig = null;
            for (DatabaseConnectionConfig dbConfig : configs) {
                PostGreSqlMetadataHandler metadata = metadataHandler(dbConfig, config, secrets, athena);
                PostGreSqlRecordHandler record = recordHandler(dbConfig, config, s3, secrets, athena);
                closeables.add(metadata);
                closeables.add(record);
                metadataMap.put(dbConfig.getCatalog(), metadata);
                recordMap.put(dbConfig.getCatalog(), record);
                if (DatabaseConnectionConfigBuilder.DEFAULT_CONNECTION_STRING_PROPERTY.equals(dbConfig.getCatalog())) {
                    metadataMap.put("lambda:" + functionName, metadata);
                    recordMap.put("lambda:" + functionName, record);
                    defaultConfig = dbConfig;
                }
            }
            if (defaultConfig == null) {
                throw new IllegalStateException("mux mode needs a default connection string");
            }
            GenericJdbcConnectionFactory factory = connectionFactory(defaultConfig, config);
            PostGreSqlMuxMetadataHandler muxMetadata = new PostGreSqlMuxMetadataHandler(secrets, athena, factory, metadataMap, defaultConfig, config);
            PostGreSqlMuxRecordHandler muxRecord = new PostGreSqlMuxRecordHandler(s3, secrets, athena, connectionFactory(defaultConfig, config), defaultConfig, recordMap, config);
            closeables.add(muxMetadata);
            composite = new CompositeHandler(muxMetadata, muxRecord);
        }
        else {
            throw new IllegalArgumentException("unknown handler mode " + mode);
        }
    }

    public static ParityHandlers create(String mode, Map<String, String> config, S3Client s3, SecretsManagerClient secrets, AthenaClient athena)
    {
        return new ParityHandlers(mode, config, s3, secrets, athena);
    }

    public CompositeHandler composite()
    {
        return composite;
    }

    private static GenericJdbcConnectionFactory connectionFactory(DatabaseConnectionConfig dbConfig, Map<String, String> config)
    {
        return new GenericJdbcConnectionFactory(dbConfig, PostGreSqlMetadataHandler.JDBC_PROPERTIES,
                new DatabaseConnectionInfo(POSTGRESQL_DRIVER_CLASS, POSTGRESQL_DEFAULT_PORT), config);
    }

    private static PostGreSqlMetadataHandler metadataHandler(DatabaseConnectionConfig dbConfig, Map<String, String> config,
            SecretsManagerClient secrets, AthenaClient athena)
    {
        if ("true".equalsIgnoreCase(config.getOrDefault("disable_spill_encryption", "false"))) {
            return new PostGreSqlMetadataHandler(dbConfig, secrets, athena, connectionFactory(dbConfig, config), config);
        }
        LocalKeyFactory keys = new LocalKeyFactory();
        return new PostGreSqlMetadataHandler(dbConfig, secrets, athena, connectionFactory(dbConfig, config), config)
        {
            @Override
            protected EncryptionKey makeEncryptionKey(AwsRequestOverrideConfiguration overrideConfiguration)
            {
                return keys.create(overrideConfiguration);
            }
        };
    }

    private static PostGreSqlRecordHandler recordHandler(DatabaseConnectionConfig dbConfig, Map<String, String> config,
            S3Client s3, SecretsManagerClient secrets, AthenaClient athena)
    {
        return new PostGreSqlRecordHandler(dbConfig, s3, secrets, athena, connectionFactory(dbConfig, config),
                new PostGreSqlQueryStringBuilder(POSTGRES_QUOTE_CHARACTER, new PostgreSqlFederationExpressionParser(POSTGRES_QUOTE_CHARACTER)), config);
    }

    @Override
    public void close()
    {
        for (AutoCloseable closeable : closeables) {
            try {
                closeable.close();
            }
            catch (Exception e) {
                System.err.println("close failed: " + e);
            }
        }
    }
}
