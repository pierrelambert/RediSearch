/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

#ifndef SQL_COMMAND_H
#define SQL_COMMAND_H

#include "redismodule.h"

/**
 * FT.SQL <sql_query>
 *
 * Execute a SQL query against RediSearch indexes.
 * The SQL query is translated to RQL (RediSearch Query Language) and executed
 * via FT.SEARCH or FT.AGGREGATE depending on the query type.
 *
 * Example:
 *   FT.SQL "SELECT * FROM idx WHERE price > 100"
 *
 * Is equivalent to:
 *   FT.SEARCH idx "@price:[(100 +inf]"
 */
int SQLCommand(RedisModuleCtx *ctx, RedisModuleString **argv, int argc);

#endif /* SQL_COMMAND_H */

