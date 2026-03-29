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
 * Query cache payload header.
 *
 * The serialized body stores the total result count followed by the cached
 * search results, including scores, flags, and row values needed to replay the
 * FT.SEARCH response on a cache hit.
 */
typedef struct CachedDocIds {
  uint32_t version;
  uint32_t reserved;
  uint64_t total_results;
  uint64_t count;
  uint8_t payload[];
} CachedDocIds;

/**
 * Serialize search results for query cache replay.
 * 
 * @param results Array of SearchResult pointers
 * @param count Number of results
 * @param lookup Lookup describing the row data schema for each result
 * @param total_results Total number of logical search results for the query
 * @param size_out Output parameter for the size of the serialized data
 * @return Pointer to allocated CachedDocIds payload (caller must free)
 */
CachedDocIds *QueryCache_SerializeDocIds(SearchResult **results, size_t count,
                                         const RLookup *lookup, uint64_t total_results,
                                         size_t *size_out);

/**
 * Deserialize cached query cache payload to search results.
 *
 * @param cached_data Pointer to cached data (CachedDocIds structure)
 * @param data_size Size of the cached data
 * @param docs DocTable to load document metadata from
 * @param lookup Lookup describing the row data schema for each result
 * @param total_results_out Output parameter for the total logical result count
 * @return Array of SearchResult pointers (caller must free with array_free)
 */
SearchResult **QueryCache_DeserializeDocIds(const uint8_t *cached_data, size_t data_size,
                                            const DocTable *docs, RLookup *lookup,
                                            uint64_t *total_results_out);

/**
 * Check if a query should be cached.
 *
 * @param reqFlags Request flags
 * @param limit Result limit
 * @return true if the query should be cached, false otherwise
 */
bool QueryCache_ShouldCache(QEFlags reqFlags, uint64_t limit);

