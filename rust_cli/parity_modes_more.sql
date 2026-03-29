.headers on
.timer off
.nullvalue NULL

-- Basic scalars + tricky strings.
select
  'a|b' as pipe,
  'a\nb' as newline,
  concat('goo', chr(0), 'se') as nul,
  'he said "hi"' as dq,
  'it''s fine' as sq;

-- Typed values + special floats.
select
  '2026-01-12 12:52:03.611328+00'::timestamptz as ts,
  nan()::double as nanv,
  (1e1000)::double as infv,
  (-1e1000)::double as ninfv;

-- Complex/nested values.
select
  [1, 2, NULL, 3] as l,
  {'a': 1, 'b': [2,3], 'c': {'d': 4}} as s,
  map(['k1','k2'], ['v1','v2']) as m;

.mode box
select 1 as one, 'x' as x;

.mode table
select 1 as one, 'x' as x;

.mode column
select 1 as one, 'x' as x;

.mode markdown
select 'a|b' as s;

.mode json
select 'nan'::double as nanv, 'inf'::double as infv, '-inf'::double as ninfv;

.mode jsonlines
select 'nan'::double as nanv, 'inf'::double as infv, '-inf'::double as ninfv;

.mode insert table
select 1 as one, 'x' as x;

.mode html
select 'a<b' as s;

.mode latex
select 1 as one, 'x' as x;

.mode ascii
select 1 as one, 'x' as x;
