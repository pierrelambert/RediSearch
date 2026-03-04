/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

#include "query_cache_integration.h"
#include "query_cache.h"
#include "config.h"
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

// Hash function for query parameters
// This is a simple FNV-1a hash - in production, you'd want something more robust
static uint64_t hash_query_params(const char *index_name, const char *query_string,
                                   size_t limit, size_t offset, const char *sort_params,
                                   const char **return_fields, size_t return_fields_count) {
  uint64_t hash = 14695981039346656037ULL; // FNV offset basis
  const uint64_t prime = 1099511628211ULL; // FNV prime

  // Hash index name
  if (index_name) {
    for (const char *p = index_name; *p; p++) {
      hash ^= (uint64_t)*p;
      hash *= prime;
    }
  }

  // Hash query string
  if (query_string) {
    for (const char *p = query_string; *p; p++) {
      hash ^= (uint64_t)*p;
      hash *= prime;
    }
  }

  // Hash limit
  hash ^= limit;
  hash *= prime;

  // Hash offset
  hash ^= offset;
  hash *= prime;

  // Hash sort params
  if (sort_params) {
    for (const char *p = sort_params; *p; p++) {
      hash ^= (uint64_t)*p;
      hash *= prime;
    }
  }

  // Hash RETURN fields count - this ensures SELECT * and SELECT name,price
  // produce different hashes even with the same query parameters
  hash ^= return_fields_count;
  hash *= prime;

  // Hash each RETURN field name
  if (return_fields) {
    for (size_t i = 0; i < return_fields_count; i++) {
      if (return_fields[i]) {
        for (const char *p = return_fields[i]; *p; p++) {
          hash ^= (uint64_t)*p;
          hash *= prime;
        }
      }
    }
  }

  return hash;
}

const uint8_t *QueryCacheIntegration_Lookup(const char *index_name, const char *query_string,
                                             size_t limit, size_t offset, const char *sort_params,
                                             const char **return_fields, size_t return_fields_count,
                                             uint64_t index_revision, size_t *size_out) {
  if (g_queryCache == NULL || size_out == NULL) {
    return NULL;
  }

  uint64_t query_hash = hash_query_params(index_name, query_string, limit, offset, sort_params,
                                           return_fields, return_fields_count);
  const uint8_t *result = QueryCache_Get(g_queryCache, query_hash, index_revision, size_out);

  // Debug logging
  fprintf(stderr, "[QueryCache] LOOKUP: idx=%s query='%s' limit=%zu offset=%zu return_count=%zu hash=%llu rev=%llu found=%d\n",
          index_name ? index_name : "NULL",
          query_string ? query_string : "NULL",
          limit, offset, return_fields_count,
          (unsigned long long)query_hash, (unsigned long long)index_revision, result != NULL);

  return result;
}

void QueryCacheIntegration_Store(const char *index_name, const char *query_string,
                                  size_t limit, size_t offset, const char *sort_params,
                                  const char **return_fields, size_t return_fields_count,
                                  uint64_t index_revision, const uint8_t *data, size_t data_size) {
  if (g_queryCache == NULL || data == NULL) {
    return;
  }

  uint64_t query_hash = hash_query_params(index_name, query_string, limit, offset, sort_params,
                                           return_fields, return_fields_count);

  // Debug logging
  fprintf(stderr, "[QueryCache] STORE: idx=%s query='%s' limit=%zu offset=%zu return_count=%zu hash=%llu rev=%llu size=%zu\n",
          index_name ? index_name : "NULL",
          query_string ? query_string : "NULL",
          limit, offset, return_fields_count,
          (unsigned long long)query_hash, (unsigned long long)index_revision, data_size);

  QueryCache_Insert(g_queryCache, query_hash, index_revision, data, data_size);
}

