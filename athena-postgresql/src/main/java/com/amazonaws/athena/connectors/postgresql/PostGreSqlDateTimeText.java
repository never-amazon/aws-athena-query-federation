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

final class PostGreSqlDateTimeText
{
    private static final long MILLIS_PER_DAY = 86_400_000L;
    private static final long UNIX_EPOCH_JDN = 2_440_588L;
    private static final long GREGORIAN_CUTOVER_DAY = -141_427L;
    private static final int MAX_FRACTION_DIGITS = 9;
    private static final int MAX_OFFSET_HOURS = 18;
    private static final int MAX_MINUTES_OR_SECONDS = 59;

    private final String text;
    private int pos;

    private PostGreSqlDateTimeText(String text)
    {
        this.text = text;
    }

    static boolean isValid(String text)
    {
        if ("infinity".equals(text) || "-infinity".equals(text)) {
            return true;
        }
        try {
            return new PostGreSqlDateTimeText(text).parse();
        }
        catch (ArithmeticException | NumberFormatException e) {
            return false;
        }
    }

    private boolean parse()
    {
        long year = 1970;
        long month = 1;
        long day = 1;
        long hour = 0;
        long minute = 0;
        long second = 0;
        long nanos = 0;
        long offsetSeconds = 0;
        boolean bc = false;
        skipWhitespace();
        boolean hasDate = false;
        boolean hasTime = false;
        int start = pos;
        Long first = number();
        if (first != null && peek() == '-') {
            hasDate = true;
            year = first;
            pos++;
            month = requiredNumber();
            if (!eat('-')) {
                return false;
            }
            day = requiredNumber();
            skipWhitespace();
        }
        else {
            pos = start;
        }
        if (isAsciiDigit(peek())) {
            hasTime = true;
            hour = requiredNumber();
            if (!eat(':')) {
                return false;
            }
            minute = requiredNumber();
            if (!eat(':')) {
                return false;
            }
            second = requiredNumber();
            if (eat('.')) {
                int digitsStart = pos;
                while (isAsciiDigit(peek())) {
                    pos++;
                }
                int digits = pos - digitsStart;
                if (digits == 0 || digits > MAX_FRACTION_DIGITS) {
                    return false;
                }
                nanos = Long.parseLong(text.substring(digitsStart, pos));
                for (int i = digits; i < MAX_FRACTION_DIGITS; i++) {
                    nanos *= 10;
                }
            }
            skipWhitespace();
        }
        char sign = peek();
        if (sign == '+' || sign == '-') {
            pos++;
            long hours = requiredNumber();
            long minutes = eat(':') ? requiredNumber() : 0;
            long seconds = eat(':') ? requiredNumber() : 0;
            if (hours > MAX_OFFSET_HOURS || minutes > MAX_MINUTES_OR_SECONDS || seconds > MAX_MINUTES_OR_SECONDS) {
                return false;
            }
            long total = hours * 3600 + minutes * 60 + seconds;
            if (total > MAX_OFFSET_HOURS * 3600) {
                return false;
            }
            offsetSeconds = sign == '-' ? -total : total;
            skipWhitespace();
        }
        if (hasDate) {
            if (text.startsWith("AD", pos)) {
                pos += 2;
            }
            else if (text.startsWith("BC", pos)) {
                bc = true;
                pos += 2;
            }
        }
        if (pos != text.length() || !(hasDate || hasTime)) {
            return false;
        }
        long epochDay = hybridEpochDay(bc ? 1 - year : year, month, day);
        long seconds = hour * 3600 + minute * 60 + second - offsetSeconds;
        Math.addExact(Math.addExact(Math.multiplyExact(epochDay, MILLIS_PER_DAY), Math.multiplyExact(seconds, 1000L)), nanos / 1_000_000);
        return true;
    }

    private char peek()
    {
        return pos < text.length() ? text.charAt(pos) : '\0';
    }

    private boolean eat(char c)
    {
        if (peek() == c) {
            pos++;
            return true;
        }
        return false;
    }

    private void skipWhitespace()
    {
        while (pos < text.length() && isAsciiWhitespace(text.charAt(pos))) {
            pos++;
        }
    }

    private static boolean isAsciiDigit(char c)
    {
        return c >= '0' && c <= '9';
    }

    private static boolean isAsciiWhitespace(char c)
    {
        return c == ' ' || c == '\t' || c == '\n' || c == '\f' || c == '\r';
    }

    private Long number()
    {
        int start = pos;
        while (isAsciiDigit(peek())) {
            pos++;
        }
        if (pos == start) {
            return null;
        }
        return (long) Integer.parseInt(text.substring(start, pos));
    }

    private long requiredNumber()
    {
        Long value = number();
        if (value == null) {
            throw new NumberFormatException("missing number at " + pos);
        }
        return value;
    }

    private static long hybridEpochDay(long year, long month, long day)
    {
        long gregorian = gregorianEpochDay(year, month, day);
        return gregorian >= GREGORIAN_CUTOVER_DAY ? gregorian : julianEpochDay(year, month, day);
    }

    private static long gregorianEpochDay(long year, long month, long day)
    {
        long a = Math.floorDiv(14 - month, 12);
        long y = year + 4800 - a;
        long m = month + 12 * a - 3;
        return day + Math.floorDiv(153 * m + 2, 5) + 365 * y + Math.floorDiv(y, 4) - Math.floorDiv(y, 100)
                + Math.floorDiv(y, 400) - 32_045 - UNIX_EPOCH_JDN;
    }

    private static long julianEpochDay(long year, long month, long day)
    {
        long a = Math.floorDiv(14 - month, 12);
        long y = year + 4800 - a;
        long m = month + 12 * a - 3;
        return day + Math.floorDiv(153 * m + 2, 5) + 365 * y + Math.floorDiv(y, 4) - 32_083 - UNIX_EPOCH_JDN;
    }
}
