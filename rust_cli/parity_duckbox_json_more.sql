.mode duckbox
.maxrows 8
.maxwidth 24

set TimeZone='UTC';

-- Deep nesting + long keys to force compact/wrap decisions.
select
  {'long_key_name': {'inner_key': [1, 2, 3, 4, 5, 6]},
   'arr': [[1,2,3], [4,5,6], [7,8,9]],
   'obj': {'a': {'b': {'c': {'d': {'e': 1}}}}}} as v;

-- Strings containing punctuation that looks structural.
select
  {'s1': 'a:b,c', 's2': '{x:[1,2]}', 's3': 'null', 's4': 'NULL'} as v;

-- JSON values that will be pretty-printed (json/variant).
select
  '[null, true, false, 1, 2.5, \"a:b,c\", {\"k\": [1,2,3]}, [4,5]]'::json as j;

-- Mix of list/struct/map, including NULLs.
select
  [ {'k': 1}, NULL, {'k': 2} ] as list_struct,
  map(['key1', 'key2', 'key3'], ['v1', NULL, 'v3']) as m;

-- Force truncation inside expanded rows.
.maxrows 6
.maxwidth 18
select
  {'k': [1, 2, 3, 4, 5, 6, 7, 8, 9, 10], 'v': [\"aaaaaaaaaa\", \"bbbbbbbbbb\", \"cccccccccc\"]} as v;

