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

