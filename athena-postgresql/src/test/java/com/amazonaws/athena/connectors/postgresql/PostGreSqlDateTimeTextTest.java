/*-
 * #%L
 * athena-postgresql
 * %%
 * Copyright (C) 2026 Amazon Web Services
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
package com.amazonaws.athena.connectors.postgresql;

import org.junit.Test;
import org.postgresql.jdbc.TimestampUtils;

import java.sql.SQLException;
import java.util.Calendar;
import java.util.TimeZone;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

public class PostGreSqlDateTimeTextTest
{
    private static final String[] MALFORMED_DATE_TIME_TEXT = {
            "2020-01-01 00:00:00+9999999999999999",
            "9223372036854775807-01-01 00:00:00",
            "2020-01-01 2562047788015216:00:00",
            "2147483647-01-01 00:00:00",
            "292278995-01-01 00:00:00",
            "2020-01-01 00:00:00+99",
            "2020-01-01 00:00:00+24:00",
            "2020-01-01 00:00:00+2147483647",
            "2020-01-01 00:00:00.9999999999",
            "2020-01-01 00:00:00x",
            "abc",
            "9223372036854775807-01-01",
    };

    private static final Object[][] ACCEPTED_DATE_TIME_TEXT = {
            {"2020-01-01 2147483647:00:00", 7_732_518_966_000_000L},
            {"292278994-08-17 07:12:55.807", Long.MAX_VALUE},
            {"2020-13-01 00:00:00", 1_609_459_200_000L},
            {"2020-01-01 00:00:00 BC", -125_882_121_600_000L},
            {"2020-01-01", 1_577_836_800_000L},
            {"2020-1-1 1:2:3", 1_577_840_523_000L},
            {"2020-01-01 00:00:00+05:30:15", 1_577_816_985_000L},
            {"infinity", 9_223_372_036_825_200_000L},
    };

    @Test
    public void rejectsTextThatPgjdbcWrapsOrRejects()
    {
        for (String text : MALFORMED_DATE_TIME_TEXT) {
            assertFalse(text, PostGreSqlDateTimeText.isValid(text));
        }
    }

    @Test
    public void acceptsLenientTextWithThePgjdbcValue() throws SQLException
    {
        TimestampUtils timestampUtils = new TimestampUtils(true, () -> TimeZone.getTimeZone("UTC"));
        for (Object[] c : ACCEPTED_DATE_TIME_TEXT) {
            String text = (String) c[0];
            assertTrue(text, PostGreSqlDateTimeText.isValid(text));
            assertEquals(text, c[1], timestampUtils.toTimestamp(Calendar.getInstance(TimeZone.getTimeZone("UTC")), text).getTime());
        }
    }
}
