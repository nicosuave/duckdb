.mode box
set timezone='America/Los_Angeles';
select '2026-01-12 12:00:00.603290+00'::timestamptz as ts;

.mode json
select 'inf'::double as inf, '-inf'::double as ninf, 'nan'::double as nan, '-nan'::double as nnan;

.mode jsonlines
select [1, 2] as a, {'x': 1} as s;

.mode markdown
select 'a|b' as v, 'x||y' as w;

.mode insert
create table t(a int);
insert into t values (1);
select * from t;
