.highlight off
.timer off

-- Wide headers/types, narrow maxwidth, and unicode width.
.headers on
.nullvalue NULL
.maxwidth 48

select
  1 as "very_long_column_name_1234567890",
  '👩‍💻' as "emoji_zwj",
  repeat('x', 80) as "long_text";

-- Multi-line values + custom nullvalue.
.nullvalue (null)
select
  'a\nb' as s,
  NULL as n,
  'a|b' as pipe;

-- Box mode quirks.
.mode box
select 1 as one, 2 as two, NULL as n;

-- Table mode quirks.
.mode table
select 1 as one, 2 as two, NULL as n;

-- Column mode quirks (alignment/padding).
.mode column
.width 6 6 6
select 1 as one, 2000 as two, NULL as n;

-- List mode + separators.
.mode list
.separator "|"
select 'a|b' as s, 'c' as t;

-- CSV mode (row separator, quoting).
.mode csv
select 'a,b' as s, 'c' as t;

-- Markdown mode (pipe escaping).
.mode markdown
select 'a|b' as s, 'c' as t;

-- JSON/JSONLINES floats + nulls.
.mode json
select 'nan'::double as nanv, 'inf'::double as infv, '-inf'::double as ninfv, NULL as n;
.mode jsonlines
select 'nan'::double as nanv, 'inf'::double as infv, '-inf'::double as ninfv, NULL as n;

-- Insert mode quoting.
.mode insert my_table
select 1 as one, 'it''s' as s, NULL as n;

-- HTML and LaTeX escaping.
.mode html
select 'a<b&c' as s;
.mode latex
select 'a_b' as s;

-- ASCII mode separators.
.mode ascii
select 1 as one, 'x' as x;
