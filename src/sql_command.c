/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

#include "sql_command.h"
#include "redismodule.h"
#include "rmalloc.h"
#include "redisearch_rs/headers/sql_parser_ffi.h"

/**
 * FT.SQL <sql_query>
 *
 * Execute a SQL query by translating it to RQL and dispatching to
 * FT.SEARCH or FT.AGGREGATE via RedisModule_Call.
 */
int SQLCommand(RedisModuleCtx *ctx, RedisModuleString **argv, int argc) {
  // FT.SQL <sql_query>
  if (argc != 2) {
    return RedisModule_WrongArity(ctx);
  }

  // Get SQL query string
  size_t sql_len;
  const char *sql = RedisModule_StringPtrLen(argv[1], &sql_len);

  // Call Rust FFI to translate SQL to RQL (with caching for performance)
  SqlTranslationResult result = sql_translate_cached(sql);

  // Handle translation error
  if (!result.success) {
    // Format error: "ERR SQL Error: <message>"
    char *error_msg = NULL;
    rm_asprintf(&error_msg, "SQL Error: %s", result.error_message);
    RedisModule_ReplyWithError(ctx, error_msg);
    rm_free(error_msg);
    sql_translation_result_free(result);
    return REDISMODULE_OK;
  }

  // Build the argument array for FT.SEARCH or FT.AGGREGATE
  // Format: FT.SEARCH <index> <query> [args...]
  // We need: index, query, + additional args (command is separate)
  int arg_count = 2 + (int)result.arguments_len;
  RedisModuleString **args = rm_malloc(sizeof(RedisModuleString *) * arg_count);

  // args[0] = index name
  args[0] = RedisModule_CreateString(ctx, result.index_name, strlen(result.index_name));

  // args[1] = query string
  args[1] = RedisModule_CreateString(ctx, result.query_string, strlen(result.query_string));

  // args[2...] = additional arguments
  for (size_t i = 0; i < result.arguments_len; i++) {
    args[2 + i] = RedisModule_CreateString(ctx, result.arguments[i], strlen(result.arguments[i]));
  }

  // Choose the command name based on the SQL statement type
  const char *cmd_name = (result.command == Search) ? "FT.SEARCH" : "FT.AGGREGATE";

  // Call the command via RedisModule_Call with "v" format (array of RedisModuleString)
  // Flags:
  //   E - return errors as RedisModuleCallReply object
  //   M - respect OOM
  //   0 - same RESP protocol as caller
  RedisModuleCallReply *reply = RedisModule_Call(ctx, cmd_name, "vEM0", args, (size_t)arg_count);

  // Forward the reply to the client
  if (reply) {
    RedisModule_ReplyWithCallReply(ctx, reply);
    RedisModule_FreeCallReply(reply);
  } else {
    // This shouldn't happen with "E" flag, but handle it anyway
    RedisModule_ReplyWithError(ctx, "ERR Failed to execute translated query");
  }

  // Free the created strings
  for (int i = 0; i < arg_count; i++) {
    RedisModule_FreeString(ctx, args[i]);
  }
  rm_free(args);

  // Free the translation result
  sql_translation_result_free(result);

  return REDISMODULE_OK;
}

