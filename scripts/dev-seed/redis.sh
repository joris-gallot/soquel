#!/bin/sh
# Dev seed: SaaS-shaped keys mirroring the sql dev seeds (sessions, cache,
# queues, counters). Re-runnable: flushes the instance first.
#   session:<hex>        10 000 strings, TTL
#   cache:user:<id>       2 000 json strings, TTL
#   user:<id>             1 000 hashes
#   queue:emails/webhooks   500 lists
#   leaderboard:tasks     1 000 zset members
#   online:users            300 set members
#   events:stream         1 000 stream entries
#   counter:api:<day>        30 strings
set -e
auth="-a soquel --no-auth-warning"

redis-cli $auth FLUSHALL > /dev/null

# j(): quote an inline-protocol argument; | stands for a json quote.
awk 'function j(s) { gsub(/\|/, "\\\\\"", s); return "\"" s "\"" }
BEGIN {
  # % 2^31: busybox awk %x clamps above INT32_MAX, which would collide keys.
  for (i = 1; i <= 10000; i++)
    printf "SET session:%08x user-%d EX %d\n", i * 2654435761 % 2147483648, i % 1000, 3600 + i % 86400
  for (i = 1; i <= 2000; i++)
    printf "SET cache:user:%d %s EX %d\n", i, j(sprintf("{|id|:%d,|plan|:|%s|,|seen|:%d}", i, (i % 5 == 0 ? "pro" : "free"), 1700000000 + i)), 300 + i % 3600
  for (i = 1; i <= 1000; i++)
    printf "HSET user:%d name user-%d email user%d@example.com plan %s logins %d\n", i, i, i, (i % 5 == 0 ? "pro" : "free"), i % 400
  for (i = 1; i <= 500; i++) {
    printf "RPUSH queue:emails %s\n", j(sprintf("{|to|:|user%d@example.com|,|template|:|digest|}", i))
    printf "RPUSH queue:webhooks %s\n", j(sprintf("{|url|:|https://hooks.example.com/%d|,|attempt|:%d}", i, i % 5))
  }
  for (i = 1; i <= 1000; i++)
    printf "ZADD leaderboard:tasks %d user-%d\n", i * 7 % 5000, i
  for (i = 1; i <= 300; i++)
    printf "SADD online:users user-%d\n", i * 3 % 1000
  for (i = 1; i <= 1000; i++)
    printf "XADD events:stream * type %s user user-%d\n", (i % 3 == 0 ? "login" : "task.done"), i % 1000
  for (i = 1; i <= 30; i++)
    printf "SET counter:api:2026-07-%02d %d\n", i, i * 1337
}' | redis-cli $auth --pipe

# One binary payload so the hex rendering shows up in the browser.
redis-cli $auth SET bin:packed "$(printf '\377\376packed-bytes\375')" > /dev/null

redis-cli $auth DBSIZE
