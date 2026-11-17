package com.amazonaws.athena.parity.wire;

import com.amazonaws.services.lambda.runtime.Context;
import com.amazonaws.services.lambda.runtime.LambdaLogger;
import software.amazon.awssdk.core.ResponseInputStream;
import software.amazon.awssdk.core.sync.RequestBody;
import software.amazon.awssdk.http.AbortableInputStream;
import software.amazon.awssdk.services.athena.AthenaClient;
import software.amazon.awssdk.services.athena.model.GetQueryExecutionResponse;
import software.amazon.awssdk.services.athena.model.QueryExecution;
import software.amazon.awssdk.services.athena.model.QueryExecutionState;
import software.amazon.awssdk.services.athena.model.QueryExecutionStatus;
import software.amazon.awssdk.services.s3.S3Client;
import software.amazon.awssdk.services.s3.model.GetObjectRequest;
import software.amazon.awssdk.services.s3.model.GetObjectResponse;
import software.amazon.awssdk.services.s3.model.HeadBucketResponse;
import software.amazon.awssdk.services.s3.model.NoSuchKeyException;
import software.amazon.awssdk.services.s3.model.PutObjectRequest;
import software.amazon.awssdk.services.s3.model.PutObjectResponse;
import software.amazon.awssdk.services.secretsmanager.SecretsManagerClient;
import software.amazon.awssdk.services.secretsmanager.model.GetSecretValueRequest;
import software.amazon.awssdk.services.secretsmanager.model.GetSecretValueResponse;
import software.amazon.awssdk.services.secretsmanager.model.ResourceNotFoundException;

import java.io.ByteArrayInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.lang.reflect.Proxy;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.TreeSet;
import java.util.concurrent.ConcurrentSkipListMap;
import java.util.concurrent.ConcurrentSkipListSet;

public final class Stubs
{
    public static final class StubCallException extends RuntimeException
    {
        public StubCallException(String message)
        {
            super(message);
        }
    }

    private final Set<String> calls = new ConcurrentSkipListSet<>();
    private final Map<String, byte[]> s3Objects = new ConcurrentSkipListMap<>();
    private final Map<String, String> secrets;

    public Stubs(Map<String, String> secrets)
    {
        this.secrets = Map.copyOf(secrets);
    }

    public Set<String> calls()
    {
        return new TreeSet<>(calls);
    }

    public Map<String, byte[]> s3Objects()
    {
        return new TreeMap<>(s3Objects);
    }

    public S3Client s3()
    {
        return (S3Client) Proxy.newProxyInstance(S3Client.class.getClassLoader(), new Class<?>[] {S3Client.class}, (proxy, method, args) -> {
            String name = method.getName();
            switch (name) {
                case "serviceName":
                    return "s3";
                case "close":
                    return null;
                case "toString":
                    return "StubS3Client";
                case "hashCode":
                    return System.identityHashCode(proxy);
                case "equals":
                    return proxy == args[0];
                default:
                    break;
            }
            calls.add("s3." + name);
            if (name.equals("headBucket")) {
                return HeadBucketResponse.builder().build();
            }
            if (name.equals("putObject") && args != null && args.length == 2 && args[0] instanceof PutObjectRequest && args[1] instanceof RequestBody) {
                PutObjectRequest request = (PutObjectRequest) args[0];
                RequestBody body = (RequestBody) args[1];
                try (InputStream in = body.contentStreamProvider().newStream()) {
                    s3Objects.put(request.bucket() + "/" + request.key(), in.readAllBytes());
                }
                return PutObjectResponse.builder().build();
            }
            if (name.equals("getObject") && args != null && args.length == 1 && args[0] instanceof GetObjectRequest) {
                GetObjectRequest request = (GetObjectRequest) args[0];
                byte[] bytes = s3Objects.get(request.bucket() + "/" + request.key());
                if (bytes == null) {
                    throw NoSuchKeyException.builder().message("stub has no object " + request.key()).build();
                }
                return new ResponseInputStream<>(GetObjectResponse.builder().contentLength((long) bytes.length).build(),
                        AbortableInputStream.create(new ByteArrayInputStream(bytes)));
            }
            throw new StubCallException("S3 stub does not implement " + name);
        });
    }

    public SecretsManagerClient secretsManager()
    {
        return (SecretsManagerClient) Proxy.newProxyInstance(SecretsManagerClient.class.getClassLoader(), new Class<?>[] {SecretsManagerClient.class}, (proxy, method, args) -> {
            String name = method.getName();
            switch (name) {
                case "serviceName":
                    return "secretsmanager";
                case "close":
                    return null;
                case "toString":
                    return "StubSecretsManagerClient";
                case "hashCode":
                    return System.identityHashCode(proxy);
                case "equals":
                    return proxy == args[0];
                default:
                    break;
            }
            calls.add("secretsmanager." + name);
            if (name.equals("getSecretValue") && args != null && args.length == 1 && args[0] instanceof GetSecretValueRequest) {
                String secretId = ((GetSecretValueRequest) args[0]).secretId();
                String value = secrets.get(secretId);
                if (value == null) {
                    throw ResourceNotFoundException.builder().message("Secrets Manager can't find the specified secret.").build();
                }
                return GetSecretValueResponse.builder().name(secretId).secretString(value).build();
            }
            throw new StubCallException("Secrets Manager stub does not implement " + name);
        });
    }

    public AthenaClient athena()
    {
        return (AthenaClient) Proxy.newProxyInstance(AthenaClient.class.getClassLoader(), new Class<?>[] {AthenaClient.class}, (proxy, method, args) -> {
            String name = method.getName();
            switch (name) {
                case "serviceName":
                    return "athena";
                case "close":
                    return null;
                case "toString":
                    return "StubAthenaClient";
                case "hashCode":
                    return System.identityHashCode(proxy);
                case "equals":
                    return proxy == args[0];
                default:
                    break;
            }
            calls.add("athena." + name);
            if (name.equals("getQueryExecution")) {
                return GetQueryExecutionResponse.builder()
                        .queryExecution(QueryExecution.builder()
                                .status(QueryExecutionStatus.builder().state(QueryExecutionState.RUNNING).build())
                                .build())
                        .build();
            }
            throw new StubCallException("Athena stub does not implement " + name);
        });
    }

    public static Context context(String requestId)
    {
        LambdaLogger logger = new LambdaLogger()
        {
            @Override
            public void log(String message)
            {
                System.err.print(message);
            }

            @Override
            public void log(byte[] message)
            {
                try {
                    System.err.write(message);
                }
                catch (IOException e) {
                    throw new IllegalStateException(e);
                }
            }
        };
        return (Context) Proxy.newProxyInstance(Context.class.getClassLoader(), new Class<?>[] {Context.class}, (proxy, method, args) -> {
            switch (method.getName()) {
                case "getAwsRequestId":
                    return requestId;
                case "getLogGroupName":
                    return "/aws/lambda/parity-pg-fn";
                case "getLogStreamName":
                    return "parity-stream";
                case "getFunctionName":
                    return "parity-pg-fn";
                case "getFunctionVersion":
                    return "$LATEST";
                case "getInvokedFunctionArn":
                    return "arn:aws:lambda:us-east-1:123456789012:function:parity-pg-fn";
                case "getRemainingTimeInMillis":
                    return 900_000;
                case "getMemoryLimitInMB":
                    return 3008;
                case "getLogger":
                    return logger;
                case "toString":
                    return "StubContext";
                case "hashCode":
                    return System.identityHashCode(proxy);
                case "equals":
                    return proxy == args[0];
                default:
                    return null;
            }
        });
    }
}
