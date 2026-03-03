/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

#pragma once

#include "redisearch.h"
#include "search_result.h"
#include "aggregate/aggregate.h"
#include <stddef.h>
#include <stdint.h>

/**
 * Phase 1 cache format: Simple array of document IDs
 * This is a minimal serialization format that caches only the document IDs,
 * requiring document data to be re-fetched from Redis on cache hit.
 */
typedef struct CachedDocIds {
  uint64_t count;      // Number of document IDs
  t_docId doc_ids[];   // Variable length array of document IDs
} CachedDocIds;

/**
 * Serialize search results to cached document IDs.
 * 
 * @param results Array of SearchResult pointers
 * @param count Number of results
 * @param size_out Output parameter for the size of the serialized data
 * @return Pointer to allocated CachedDocIds structure (caller must free)
 */
CachedDocIds *QueryCache_SerializeDocIds(SearchResult **results, size_t count, size_t *size_out);

/**
 * Check if a query should be cached.
 * 
 * @param reqFlags Request flags
 * @param limit Result limit
 * @return true if the query should be cached, false otherwise
 */
bool QueryCache_ShouldCache(QEFlags reqFlags, uint64_t limit);

