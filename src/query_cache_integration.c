/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

#include "query_cache_integration.h"
#include "aggregate/aggregate.h"
#include "query_cache.h"
#include "util/fnv.h"
#include <string.h>

// Global query cache instance
static QueryCache *g_queryCache = NULL;

void QueryCacheIntegration_Init(size_t max_entries) {
  if (g_queryCache != NULL) {
    QueryCache_Free(g_queryCache);
  }
  g_queryCache = QueryCache_New(max_entries);
}

void QueryCacheIntegration_Shutdown(void) {
  if (g_queryCache != NULL) {
    QueryCache_Free(g_queryCache);
    g_queryCache = NULL;
  }
}

QueryCache *QueryCacheIntegration_Get(void) {
  return g_queryCache;
}

void QueryCacheIntegration_Resize(size_t new_max_entries) {
  if (g_queryCache != NULL) {
    QueryCache_Resize(g_queryCache, new_max_entries);
  }
}

void QueryCacheIntegration_Clear(void) {
  if (g_queryCache != NULL) {
    QueryCache_Clear(g_queryCache);
  }
}

QueryCacheStats QueryCacheIntegration_GetStats(void) {
  if (g_queryCache != NULL) {
    return QueryCache_GetStats(g_queryCache);
  }
  
  // Return empty stats if cache not initialized
  QueryCacheStats empty = {0};
  return empty;
}

static uint64_t QueryCache_HashBytes(uint64_t hash, const void *buf, size_t len) {
  uint64_t serialized_len = len;
  hash = fnv_64a_buf(&serialized_len, sizeof(serialized_len), hash);
  return len ? fnv_64a_buf(buf, len, hash) : hash;
}

static uint64_t QueryCache_HashString(uint64_t hash, const char *value) {
  return value ? QueryCache_HashBytes(hash, value, strlen(value)) : QueryCache_HashBytes(hash, "", 0);
}

static uint64_t hash_query_params(const char *index_name, const AREQ *req) {
  uint64_t hash = 14695981039346656037ULL;
  size_t nargs = req ? req->nargs : 0;
  unsigned int dialect = req ? req->reqConfig.dialectVersion : 0;
  unsigned int tanh_factor = req ? req->reqConfig.BM25STD_TanhFactor : 0;

  hash = QueryCache_HashString(hash, index_name);
  hash = QueryCache_HashBytes(hash, &nargs, sizeof(nargs));
  hash = QueryCache_HashBytes(hash, &dialect, sizeof(dialect));
  hash = QueryCache_HashBytes(hash, &tanh_factor, sizeof(tanh_factor));

  if (!req) {
    return hash;
  }

  for (size_t i = 0; i < req->nargs; ++i) {
    const sds arg = req->args[i];
    size_t arg_len = arg ? sdslen(arg) : 0;
    hash = QueryCache_HashBytes(hash, arg ? arg : "", arg_len);
  }

  hash = QueryCache_HashString(hash, req->searchopts.scorerName);
  hash = QueryCache_HashString(hash, req->searchopts.expanderName);
  return hash;
}

const uint8_t *QueryCacheIntegration_Lookup(const char *index_name, const AREQ *req,
                                            uint64_t index_revision, size_t *size_out) {
  if (g_queryCache == NULL || size_out == NULL) {
    return NULL;
  }

  uint64_t query_hash = hash_query_params(index_name, req);
  const uint8_t *result = QueryCache_Get(g_queryCache, query_hash, index_revision, size_out);
  return result;
}

void QueryCacheIntegration_Store(const char *index_name, const AREQ *req,
                                 uint64_t index_revision, const uint8_t *data, size_t data_size) {
  if (g_queryCache == NULL || data == NULL) {
    return;
  }

  uint64_t query_hash = hash_query_params(index_name, req);
  QueryCache_Insert(g_queryCache, query_hash, index_revision, data, data_size);
}

