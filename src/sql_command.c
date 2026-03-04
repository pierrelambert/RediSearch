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
#include <string.h>
#include <stdlib.h>

/**
 * Convert a JSON-style vector string to binary float32 format.
 * Input: "[0.1, 0.2, 0.3]"
 * Output: Binary blob of 3 float32 values
 *
 * Returns the binary blob (caller must free) and sets out_size.
 * Returns NULL on parse error.
 */
static char *vector_string_to_blob(const char *vector_str, size_t *out_size) {
  if (!vector_str || vector_str[0] != '[') {
    return NULL;
  }

  // Count elements (approximate by counting commas + 1)
  size_t capacity = 16;
  float *floats = rm_malloc(sizeof(float) * capacity);
  size_t count = 0;

  const char *p = vector_str + 1;  // Skip '['
  while (*p && *p != ']') {
    // Skip whitespace
    while (*p == ' ' || *p == '\t' || *p == '\n') p++;
    if (*p == ']' || *p == '\0') break;

    // Parse float
    char *end;
    float val = strtof(p, &end);
    if (end == p) {
      // Parse error
      rm_free(floats);
      return NULL;
    }

    // Store value
    if (count >= capacity) {
      capacity *= 2;
      floats = rm_realloc(floats, sizeof(float) * capacity);
    }
    floats[count++] = val;

    // Move to next element
    p = end;
    while (*p == ' ' || *p == '\t' || *p == '\n') p++;
    if (*p == ',') p++;
  }

  if (count == 0) {
    rm_free(floats);
    return NULL;
  }

  *out_size = count * sizeof(float);
  return (char *)floats;
}

/**
 * Check if an argument looks like a vector string (starts with '[').
 */
static int is_vector_string(const char *str) {
  return str && str[0] == '[';
}

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
  // Format: FT.SEARCH <index> <query> [args...] DIALECT 2
  // We need: index, query, + additional args + DIALECT 2 (command is separate)
  // Always add DIALECT 2 to enable features like ismissing() for IS NULL queries
  int arg_count = 2 + (int)result.arguments_len + 2;  // +2 for DIALECT 2
  RedisModuleString **args = rm_malloc(sizeof(RedisModuleString *) * arg_count);

  // args[0] = index name
  args[0] = RedisModule_CreateString(ctx, result.index_name, strlen(result.index_name));

  // args[1] = query string
  args[1] = RedisModule_CreateString(ctx, result.query_string, strlen(result.query_string));

  // args[2...] = additional arguments
  // Track binary blobs that need freeing
  char *vector_blob = NULL;
  for (size_t i = 0; i < result.arguments_len; i++) {
    const char *arg = result.arguments[i];
    // If this is a vector string (starts with '['), convert to binary blob
    if (is_vector_string(arg)) {
      size_t blob_size;
      vector_blob = vector_string_to_blob(arg, &blob_size);
      if (vector_blob) {
        args[2 + i] = RedisModule_CreateString(ctx, vector_blob, blob_size);
      } else {
        // Fallback: pass as-is (will likely error at FT.SEARCH level)
        args[2 + i] = RedisModule_CreateString(ctx, arg, strlen(arg));
      }
    } else {
      args[2 + i] = RedisModule_CreateString(ctx, arg, strlen(arg));
    }
  }

  // Add DIALECT 2 to enable advanced features like ismissing() for IS NULL queries
  size_t dialect_idx = 2 + result.arguments_len;
  args[dialect_idx] = RedisModule_CreateString(ctx, "DIALECT", 7);
  args[dialect_idx + 1] = RedisModule_CreateString(ctx, "2", 1);

  // Choose the command name based on the SQL statement type.
  // Call the public FT.* commands which handle both single-shard and multi-shard deployments.
  const char *cmd_name;
  switch (result.command) {
    case Search:
      cmd_name = "FT.SEARCH";
      break;
    case Aggregate:
      cmd_name = "FT.AGGREGATE";
      break;
    case Hybrid:
      cmd_name = "FT.HYBRID";
      break;
    default:
      cmd_name = "FT.SEARCH";
      break;
  }

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

  // Free vector blob if allocated
  if (vector_blob) {
    rm_free(vector_blob);
  }

  // Free the translation result
  sql_translation_result_free(result);

  return REDISMODULE_OK;
}

