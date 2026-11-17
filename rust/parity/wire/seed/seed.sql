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
