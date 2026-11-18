package com.amazonaws.athena.connector.lambda.metadata.glue;

/*-
 * #%L
 * Amazon Athena Query Federation SDK
 * %%
 * Copyright (C) 2019 Amazon Web Services
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

import com.amazonaws.athena.connector.lambda.exceptions.AthenaConnectorException;
import com.google.common.collect.ImmutableMap;
import com.google.common.collect.ImmutableSet;
import org.apache.arrow.vector.types.Types;
import org.apache.arrow.vector.types.pojo.ArrowType;
import software.amazon.awssdk.services.glue.model.ErrorDetails;
import software.amazon.awssdk.services.glue.model.FederationSourceErrorCode;

import java.time.ZoneId;
import java.util.Arrays;
import java.util.Map;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * Defines the default mapping of AWS Glue Data Catalog types to Apache Arrow types. You can override these by
 * overriding convertField(...) on GlueMetadataHandler.
 */
public class DefaultGlueType
{
    private static final String TIMESTAMPMILLITZ = "timestamptz";
    private static final Set<String> NON_COMPARABALE_SET = ImmutableSet.of("TIMESTAMPMILLITZ");

    private static final Map<String, ArrowType> TYPE_MAP = new ImmutableMap.Builder<String, ArrowType>()
        .put("int", Types.MinorType.INT.getType())
        .put("varchar", Types.MinorType.VARCHAR.getType())
        .put("string", Types.MinorType.VARCHAR.getType())
        .put("bigint", Types.MinorType.BIGINT.getType())
        .put("double", Types.MinorType.FLOAT8.getType())
        .put("float", Types.MinorType.FLOAT4.getType())
        .put("smallint", Types.MinorType.SMALLINT.getType())
        .put("tinyint", Types.MinorType.TINYINT.getType())
        .put("boolean", Types.MinorType.BIT.getType())
        .put("binary", Types.MinorType.VARBINARY.getType())
        .put("timestamp", Types.MinorType.DATEMILLI.getType())
        .put("date", Types.MinorType.DATEDAY.getType())
        // ZoneId.systemDefault().getId() is just a place holder, each row will have a TZ value
        // otherwise fall back to the table configured default TZ
        .put(TIMESTAMPMILLITZ, (ArrowType) new ArrowType.Timestamp(
                org.apache.arrow.vector.types.TimeUnit.MILLISECOND, ZoneId.systemDefault().getId()))
        .build();

    private static final int DECIMAL_128_BIT_WIDTH = 128;
    private static final int DECIMAL_256_BIT_WIDTH = 256;
    private static final int DECIMAL_128_MAX_PRECISION = 38;
    private static final int DECIMAL_256_MAX_PRECISION = 76;

    // decimal match examples:
    // decimal
    // decimal(1,2)
    // decimal(3,2,1)
    private static final Pattern decimalPattern = Pattern.compile("decimal([(](([0-9]+,?){2,3})[)])?");

    private String id;
    private ArrowType arrowType;

    DefaultGlueType(String id, ArrowType arrowType)
    {
        this.id = id;
        this.arrowType = arrowType;
    }

    private static ArrowType getDecimalArrowType(String in)
    {
        Matcher decimalMatcher = decimalPattern.matcher(in);
        if (!decimalMatcher.matches()) {
            return null;
        }
        try {
            int[] params = Arrays.stream(decimalMatcher.group(2).split(","))
                    .mapToInt(Integer::parseInt).toArray();
            if (params.length == 2) {
                return checkedDecimal(in, params[0], params[1], DECIMAL_128_BIT_WIDTH);
            }
            // else this must be 3 because of the regex
            return checkedDecimal(in, params[0], params[1], params[2]);
        }
        catch (java.lang.NullPointerException e) {
            // This is the case where it is only "decimal" with no parameters
            // Using the default precision and scale that spark sql defaults
            // to when no parameters are specified.
            // NOTE: I would prefer to check the decimalMatcher.groupCount() above
            // rather than a try catch but decimalMatcher.groupCount() always returns
            // 3 for some reason...
            return new ArrowType.Decimal(38, 18);
        }
    }

    private static ArrowType checkedDecimal(String in, int precision, int scale, int bitWidth)
    {
        if (bitWidth != DECIMAL_128_BIT_WIDTH && bitWidth != DECIMAL_256_BIT_WIDTH) {
            throw new IllegalArgumentException("Library only supports 128-bit and 256-bit decimal values");
        }
        int maxPrecision = bitWidth == DECIMAL_128_BIT_WIDTH ? DECIMAL_128_MAX_PRECISION : DECIMAL_256_MAX_PRECISION;
        if (precision < 1 || precision > maxPrecision || scale > precision) {
            throw new IllegalArgumentException(String.format(
                    "Invalid decimal type %s: a %d-bit decimal needs a precision from 1 to %d and a scale no larger than the precision",
                    in, bitWidth, maxPrecision));
        }
        return new ArrowType.Decimal(precision, scale, bitWidth);
    }

    public static ArrowType fromId(String id)
    {
        ArrowType result = toArrowType(id);
        if (result == null) {
            throw new AthenaConnectorException("Unknown DefaultGlueType for id: " + id, ErrorDetails.builder().errorCode(FederationSourceErrorCode.INVALID_INPUT_EXCEPTION.toString()).build());
        }
        return result;
    }

    public static ArrowType toArrowType(String id)
    {
        ArrowType result = TYPE_MAP.get(id.toLowerCase());
        if (result == null) {
            return getDecimalArrowType(id);
        }

        return result;
    }

    public ArrowType getArrowType()
    {
        return arrowType;
    }

    public static Set<String> getNonComparableSet()
    {
        return NON_COMPARABALE_SET;
    }
}
