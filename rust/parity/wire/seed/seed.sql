SET TIME ZONE 'UTC';

CREATE SCHEMA parity;
CREATE SCHEMA parity_pages;
CREATE SCHEMA "MixedSchema";
CREATE SCHEMA empty_schema;

CREATE TABLE parity.all_types (
    id integer PRIMARY KEY,
    smallint_col smallint,
    int_col integer,
    bigint_col bigint,
    numeric_scaled numeric(18, 4),
    numeric_noscale numeric(10),
    numeric_unbounded numeric,
    real_col real,
    double_col double precision,
    text_col text,
    varchar_col varchar(50),
    char_col char(10),
    bool_col boolean,
    date_col date,
    ts_col timestamp,
    tstz_col timestamptz,
    bytea_col bytea,
    int_arr integer[],
    bigint_arr bigint[],
    text_arr text[],
    float8_arr double precision[],
    bool_arr boolean[],
    date_arr date[],
    numeric_arr numeric(10, 2)[],
    uuid_col uuid,
    json_col json,
    time_col time
);

INSERT INTO parity.all_types
SELECT
    g,
    CASE WHEN g % 10 = 0 THEN NULL ELSE ((g % 32000) - 16000)::smallint END,
    CASE WHEN g % 10 = 1 THEN NULL ELSE g * 7 - 5000 END,
    CASE WHEN g % 10 = 2 THEN NULL ELSE (g::bigint * 1000003) - 1500000000 END,
    CASE WHEN g % 10 = 3 THEN NULL ELSE round((g * 12.3457)::numeric, 4) END,
    CASE WHEN g % 10 = 4 THEN NULL ELSE (g * 31)::numeric(10) END,
    CASE WHEN g % 10 = 5 THEN NULL ELSE (g / 8.0)::numeric END,
    CASE WHEN g % 10 = 6 THEN NULL ELSE (g * 0.25)::real END,
    CASE WHEN g % 10 = 7 THEN NULL ELSE (g * 1.125 - 100)::double precision END,
    CASE WHEN g % 10 = 8 THEN NULL ELSE 'text_' || lpad(g::text, 5, '0') || CASE WHEN g % 7 = 0 THEN ' unicode é漢' ELSE '' END END,
    CASE WHEN g % 10 = 9 THEN NULL ELSE (ARRAY['alpha', 'beta', 'gamma', 'delta', 'epsilon'])[(g % 5) + 1] END,
    CASE WHEN g % 11 = 0 THEN NULL ELSE ('c' || (g % 13))::char(10) END,
    CASE WHEN g % 12 = 0 THEN NULL ELSE (g % 2 = 0) END,
    CASE WHEN g % 13 = 0 THEN NULL ELSE DATE '1960-01-01' + (g * 11) END,
    CASE WHEN g % 14 = 0 THEN NULL ELSE TIMESTAMP '1965-06-15 08:30:00' + (g * INTERVAL '1 day 3 hours 7 minutes 11.123 seconds') END,
    CASE WHEN g % 15 = 0 THEN NULL ELSE TIMESTAMPTZ '2000-01-01 00:00:00+00' + (g * INTERVAL '5 hours 17 minutes') END,
    CASE WHEN g % 16 = 0 THEN NULL ELSE decode(lpad(to_hex(g), 8, '0'), 'hex') END,
    CASE WHEN g % 17 = 0 THEN NULL WHEN g % 17 = 1 THEN ARRAY[]::integer[] ELSE ARRAY[g, g + 1, g + 2] END,
    CASE WHEN g % 18 = 0 THEN NULL ELSE ARRAY[g::bigint * 100000, g::bigint * -1] END,
    CASE WHEN g % 19 = 0 THEN NULL ELSE ARRAY['a' || g, 'b' || g] END,
    CASE WHEN g % 20 = 0 THEN NULL ELSE ARRAY[g * 0.5, g * 1.5]::double precision[] END,
    CASE WHEN g % 21 = 0 THEN NULL ELSE ARRAY[g % 2 = 0, g % 3 = 0] END,
    CASE WHEN g % 22 = 0 THEN NULL ELSE ARRAY[DATE '2020-01-01' + g, DATE '1969-12-31' - g] END,
    CASE WHEN g % 23 = 0 THEN NULL ELSE ARRAY[(g * 1.25)::numeric(10, 2), (g * -0.75)::numeric(10, 2)] END,
    CASE WHEN g % 24 = 0 THEN NULL ELSE ('00000000-0000-0000-0000-' || lpad(to_hex(g), 12, '0'))::uuid END,
    CASE WHEN g % 25 = 0 THEN NULL ELSE json_build_object('k', g, 'v', 'x' || g) END,
    CASE WHEN g % 26 = 0 THEN NULL ELSE TIME '00:00:00' + (g * INTERVAL '37 seconds') END
FROM generate_series(1, 3000) AS g;

CREATE TABLE parity.sales (
    sale_id integer NOT NULL,
    region varchar(20),
    sale_date date NOT NULL,
    amount numeric(12, 2),
    note text
) PARTITION BY RANGE (sale_date);

CREATE TABLE parity.sales_2023 PARTITION OF parity.sales FOR VALUES FROM ('2023-01-01') TO ('2024-01-01');
CREATE TABLE parity.sales_2024 PARTITION OF parity.sales FOR VALUES FROM ('2024-01-01') TO ('2025-01-01');
CREATE TABLE parity.sales_other PARTITION OF parity.sales DEFAULT;

INSERT INTO parity.sales
SELECT
    g,
    (ARRAY['north', 'south', 'east', 'west'])[(g % 4) + 1],
    DATE '2022-07-01' + (g % 900),
    CASE WHEN g % 9 = 0 THEN NULL ELSE round((g * 3.17)::numeric, 2) END,
    CASE WHEN g % 5 = 0 THEN NULL ELSE 'note ' || g END
FROM generate_series(1, 1200) AS g;

CREATE VIEW parity.all_types_view AS
    SELECT id, int_col, text_col, date_col, numeric_scaled FROM parity.all_types WHERE id <= 500;

CREATE MATERIALIZED VIEW parity.sales_summary_mv AS
    SELECT region, count(*)::bigint AS sale_count, sum(amount) AS total_amount
    FROM parity.sales GROUP BY region ORDER BY region;

CREATE TABLE parity.small_pk (
    id integer PRIMARY KEY,
    label text
);
INSERT INTO parity.small_pk SELECT g, 'label ' || g FROM generate_series(1, 10) AS g;

CREATE TABLE parity.uuid_pk (
    id uuid PRIMARY KEY,
    payload text
);
INSERT INTO parity.uuid_pk
SELECT ('00000000-0000-0000-0000-' || lpad(to_hex(g), 12, '0'))::uuid, 'payload ' || g FROM generate_series(1, 50) AS g;

CREATE TABLE parity.no_pk (
    a integer,
    b text
);
INSERT INTO parity.no_pk SELECT g, 'b' || g FROM generate_series(1, 40) AS g;

CREATE TABLE parity.empty_table (
    id integer PRIMARY KEY,
    v text
);

CREATE TABLE parity."CamelTable" (
    "Id" integer PRIMARY KEY,
    "CamelValue" text,
    lower_value integer
);
INSERT INTO parity."CamelTable" SELECT g, 'Camel' || g, g * 2 FROM generate_series(1, 30) AS g;

CREATE TABLE "MixedSchema"."MixedTable" (
    "MixedId" integer PRIMARY KEY,
    "MixedText" text,
    "UPPER_COL" double precision
);
INSERT INTO "MixedSchema"."MixedTable" SELECT g, 'Mixed ' || g, g * 0.5 FROM generate_series(1, 25) AS g;

CREATE TABLE "MixedSchema".lower_table (
    id integer,
    v text
);
INSERT INTO "MixedSchema".lower_table VALUES (1, 'one'), (2, 'two');

DO $$
BEGIN
    FOR i IN 1..25 LOOP
        EXECUTE format('CREATE TABLE parity_pages.%I (id integer)', 'page_t' || lpad(i::text, 2, '0'));
    END LOOP;
END
$$;
CREATE VIEW parity_pages.page_view AS SELECT 1 AS one;

ANALYZE;

CREATE TABLE parity.edges (LIKE parity.all_types INCLUDING ALL);
INSERT INTO parity.edges (id, smallint_col, int_col, bigint_col, numeric_scaled, numeric_noscale, numeric_unbounded, real_col, double_col) VALUES
 (1, -32768, -2147483648, -9223372036854775808, 99999999999999.9999, 9999999999, 12345678901234567890123456789012345678, 'NaN', 'Infinity'),
 (2, 32767, 2147483647, 9223372036854775807, -99999999999999.9999, -9999999999, -0.5, '-Infinity', '-0'),
 (3, 0, 0, 0, 0.0001, 0, 0.000000000000000000000000000001, 'Infinity', 'NaN'),
 (4, NULL, NULL, NULL, NULL, NULL, 1e40, 3.4028235e38, 1e308),
 (5, NULL, NULL, NULL, NULL, NULL, -1e40, 1.4e-45, 4.9e-324),
 (6, NULL, NULL, NULL, NULL, NULL, 'NaN', NULL, NULL),
 (7, NULL, NULL, NULL, NULL, NULL, 2.5, NULL, NULL),
 (8, NULL, NULL, NULL, NULL, NULL, -2.5, NULL, NULL);
INSERT INTO parity.edges (id, date_col, ts_col, tstz_col) VALUES
 (20, '1000-01-01', '1000-01-01 12:00:00', '1000-01-01 00:00:00+00'),
 (21, '0001-01-01 BC', '0044-03-15 12:00:00 BC', '0001-01-01 00:00:00+00 BC'),
 (22, '1582-10-04', '1582-10-04 23:59:59.999', '1582-10-15 00:00:00+00'),
 (23, '1969-12-31', '1969-12-31 23:59:59.999999', '1969-12-31 23:59:59.9995+00'),
 (24, '2020-06-01', '2020-06-01 12:00:00.123456', '2020-06-01 12:00:00.987654+05:30'),
 (25, '9999-12-31', '9999-12-31 23:59:59.999', '2038-01-19 03:14:08-08:00'),
 (26, 'infinity', NULL, NULL),
 (27, '-infinity', NULL, NULL),
 (28, NULL, 'infinity', NULL),
 (29, NULL, NULL, '-infinity'),
 (30, '10000-01-01', '10000-01-01 00:00:00', NULL),
 (31, '1900-01-01', '1900-01-01 00:00:00', '1900-01-01 00:00:00-04:56:02');
INSERT INTO parity.edges (id, text_col, varchar_col, char_col, bytea_col, uuid_col, json_col, time_col) VALUES
 (40, '', '', '', '\x', '00000000-0000-0000-0000-000000000000', '{}', '00:00:00'),
 (41, E'it''s \\ "q" é漢 \t\n', 'O''Brien', 'a', '\x00ff7f80', 'ffffffff-ffff-ffff-ffff-ffffffffffff', '[1, "x", null]', '24:00:00'),
 (42, repeat('long', 20000), NULL, NULL, decode(repeat('ab', 70000), 'hex'), NULL, '"é"', '23:59:59.999999');
INSERT INTO parity.edges (id, int_arr, bigint_arr, text_arr, float8_arr, bool_arr, date_arr, numeric_arr) VALUES
 (60, '{}', '{}', '{}', '{}', '{}', '{}', '{}'),
 (61, '{NULL}', '{NULL,1}', '{NULL,"","a,b","NULL","q\"x"}', '{NaN,Infinity,-Infinity,-0}', '{NULL,true}', '{1970-01-01,NULL}', '{NULL,1.5}'),
 (62, '{{1,2},{3,4}}', NULL, NULL, NULL, NULL, NULL, NULL),
 (63, NULL, NULL, NULL, NULL, NULL, '{infinity}', NULL),
 (64, NULL, NULL, NULL, NULL, NULL, NULL, '{NaN}'),
 (65, '[0:1]={7,8}', '{-9223372036854775808,9223372036854775807}', '{" lead","trail ","é"}', '{1e308,4.9e-324}', NULL, '{0001-01-01 BC}', '{99999999.99}');
INSERT INTO parity.edges (id) VALUES (80);

CREATE TABLE parity.odd_types (id integer PRIMARY KEY, m money, iv interval, n52 numeric(5,2), ts3 timestamp(3), tstz0 timestamptz(0), vc varchar, ch char, i2arr smallint[], f4 real[], ja jsonb, ip inet, bits bit(4), xm xml, en text);
INSERT INTO parity.odd_types VALUES
 (1, 1.5, '1 year 2 mons 3 days 04:05:06.789', 123.45, '2020-01-01 00:00:00.123', '2020-01-01 00:00:00+00', 'v', 'c', '{1,-1}', '{1.5,NaN}', '{"a":1}', '10.0.0.1/8', B'1010', '<a/>', 'x'),
 (2, -92233720368547758.08, '-1 days', -999.99, NULL, NULL, NULL, NULL, '{}', NULL, NULL, NULL, NULL, NULL, NULL),
 (3, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL);

CREATE TABLE parity.unicode_text (LIKE parity.all_types INCLUDING ALL);
INSERT INTO parity.unicode_text (id, text_col, varchar_col) VALUES
 (1, '日本語', '日本語'),
 (2, '😀 emoji', '😀'),
 (3, 'ascii', 'ascii'),
 (4, 'café', 'café'),
 (5, 'it''s 日本', 'x 😀 y'),
 (6, NULL, NULL);

CREATE TABLE parity.big (LIKE parity.all_types INCLUDING ALL);
INSERT INTO parity.big SELECT a.id + k * 3000, a.smallint_col, a.int_col, a.bigint_col, a.numeric_scaled, a.numeric_noscale, a.numeric_unbounded, a.real_col, a.double_col, a.text_col, a.varchar_col, a.char_col, a.bool_col, a.date_col, a.ts_col, a.tstz_col, a.bytea_col, a.int_arr, a.bigint_arr, a.text_arr, a.float8_arr, a.bool_arr, a.date_arr, a.numeric_arr, a.uuid_col, a.json_col, a.time_col
FROM parity.all_types a CROSS JOIN generate_series(0, 66) k;
ANALYZE parity.big;

CREATE SCHEMA parity_cal;
CREATE TABLE parity_cal.calendar (id integer PRIMARY KEY, d date, ts timestamp, tstz timestamptz, ts_arr timestamp[], d_arr date[], tstz_arr timestamptz[]);
INSERT INTO parity_cal.calendar VALUES
 (1, '1000-01-01', '1000-01-01 12:00:00', '1000-01-01 12:00:00+00', '{"1000-01-01 12:00:00","1582-10-04 23:59:59.999"}', '{1000-01-01,1582-10-04}', '{"1000-01-01 12:00:00+00","1582-10-04 23:59:59.999+00"}'),
 (2, '0044-03-15 BC', '0044-03-15 12:00:00 BC', '0044-03-15 12:00:00+00 BC', '{"0044-03-15 12:00:00 BC"}', '{"0044-03-15 BC","0001-01-01 BC"}', '{"0044-03-15 12:00:00+00 BC"}'),
 (3, '0001-02-29 BC', '0001-02-29 06:30:00.5 BC', '0001-02-29 06:30:00.5+00 BC', '{"0001-02-29 06:30:00.5 BC"}', '{"0001-02-29 BC"}', '{"0001-02-29 06:30:00.5+00 BC"}'),
 (4, '1582-10-15', '1582-10-15 00:00:00', '1582-10-15 00:00:00+00', '{"1582-10-15 00:00:00",NULL}', '{1582-10-15,NULL}', '{"1582-10-15 00:00:00+00",NULL}'),
 (5, '2020-06-01', '2020-06-01 12:00:00.123', '2020-06-01 12:00:00.123+05:30', '{"2020-06-01 12:00:00.123",infinity}', '{2020-06-01,infinity}', '{"2020-06-01 12:00:00.123+05:30",infinity,-infinity}'),
 (6, NULL, NULL, NULL, NULL, NULL, NULL);
ANALYZE parity_cal.calendar;
