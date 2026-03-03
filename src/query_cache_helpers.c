/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

#include "query_cache_helpers.h"
#include "aggregate/aggregate.h"
#include "rmutil/alloc.h"
#include "util/arr.h"
#include <string.h>

CachedDocIds *QueryCache_SerializeDocIds(SearchResult **results, size_t count, size_t *size_out) {
  if (!results || count == 0 || !size_out) {
    return NULL;
  }

  // Calculate size: header + array of doc IDs
  size_t data_size = sizeof(CachedDocIds) + count * sizeof(t_docId);
  CachedDocIds *cached = rm_malloc(data_size);
  if (!cached) {
    return NULL;
  }

  cached->count = count;

  // Extract document IDs from results
  for (size_t i = 0; i < count; i++) {
    cached->doc_ids[i] = SearchResult_GetDocId(results[i]);
  }

  *size_out = data_size;
  return cached;
}

SearchResult **QueryCache_DeserializeDocIds(const uint8_t *cached_data, size_t data_size) {
  if (!cached_data || data_size < sizeof(CachedDocIds)) {
    return NULL;
  }

  const CachedDocIds *cached = (const CachedDocIds *)cached_data;

  // Validate data size
  size_t expected_size = sizeof(CachedDocIds) + cached->count * sizeof(t_docId);
  if (data_size != expected_size) {
    return NULL;
  }

  // Create array of SearchResult pointers
  SearchResult **results = array_new(SearchResult *, cached->count);
  if (!results) {
    return NULL;
  }

  // Create SearchResult for each cached doc ID
  for (size_t i = 0; i < cached->count; i++) {
    SearchResult *res = rm_malloc(sizeof(SearchResult));
    if (!res) {
      // Cleanup on allocation failure
      for (size_t j = 0; j < i; j++) {
        SearchResult_Destroy(results[j]);
        rm_free(results[j]);
      }
      array_free(results);
      return NULL;
    }

    *res = SearchResult_New();
    SearchResult_SetDocId(res, cached->doc_ids[i]);
    array_append(results, res);
  }

  return results;
}

bool QueryCache_ShouldCache(QEFlags reqFlags, uint64_t limit) {
  // Don't cache cursor queries (stateful, partial results)
  if (reqFlags & QEXEC_F_IS_CURSOR) {
    return false;
  }
  
  // Don't cache unlimited queries (LIMIT 0 0 or very large limits)
  if (limit == UINT64_MAX || limit == 0) {
    return false;
  }
  
  return true;
}

