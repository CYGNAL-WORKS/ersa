-- Migration 12: section names are compared exactly, as Claude Desktop
-- compares them (PROTO-CORE-API-R76): names that differ only in case are
-- different sections. `name_key` keeps its unique key and now holds the
-- name itself. Every key so far is its name in lowercase and no two are
-- alike, so no two names are alike either and this cannot conflict.
UPDATE sections SET name_key = name;
