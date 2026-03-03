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
#include "doc_table.h"
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

SearchResult **QueryCache_DeserializeDocIds(const uint8_t *cached_data, size_t data_size, const DocTable *docs) {
  if (!cached_data || data_size < sizeof(CachedDocIds) || !docs) {
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
    t_docId doc_id = cached->doc_ids[i];

    // Load document metadata from DocTable
    const RSDocumentMetadata *dmd = DocTable_Borrow(docs, doc_id);
    if (!dmd) {
      // Document was deleted or doesn't exist - cleanup and return NULL
      for (size_t j = 0; j < i; j++) {
        const RSDocumentMetadata *prev_dmd = SearchResult_GetDocumentMetadata(results[j]);
        if (prev_dmd) {
          DMD_Return(prev_dmd);
        }
        SearchResult_Destroy(results[j]);
        rm_free(results[j]);
      }
      array_free(results);
      return NULL;
    }

    SearchResult *res = rm_malloc(sizeof(SearchResult));
    if (!res) {
      // Cleanup on allocation failure
      DMD_Return(dmd);
      for (size_t j = 0; j < i; j++) {
        const RSDocumentMetadata *prev_dmd = SearchResult_GetDocumentMetadata(results[j]);
        if (prev_dmd) {
          DMD_Return(prev_dmd);
        }
        SearchResult_Destroy(results[j]);
        rm_free(results[j]);
      }
      array_free(results);
      return NULL;
    }

    *res = SearchResult_New();
    SearchResult_SetDocId(res, doc_id);
    SearchResult_SetDocumentMetadata(res, dmd);
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

