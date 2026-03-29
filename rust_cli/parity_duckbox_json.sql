.mode duckbox
.maxrows 20
.maxwidth 40

set TimeZone='UTC';

select
  {'a': [42, 999, NULL, -42],
   'b': {'c': [1, 2, 3], 'd': 'x'},
   'e': [{'k': 1}, {'k': 2}],
   'f': map(['key1', 'key2'], ['v1', 'v2']),
   'g': '[1,2,{\"a\":3,\"b\":[4,5]}]'::json,
   'h': ['a', 'b', 'c']} as v;

select map(['key1','key2'], ['v1','v2']) as m;
select [{'k': 1}, {'k': 2}] as list_of_struct;
select '[1,2,{\"a\":3,\"b\":[4,5]}]'::json as j;

