/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

#pragma once

#include <stddef.h>
#include <stdint.h>

// Forward declaration
typedef struct QueryCache QueryCache;
typedef struct QueryCacheStats QueryCacheStats;

/**
 * Initialize the global query cache with the specified maximum number of entries.
 * 
 * @param max_entries Maximum number of cache entries (default: 1000)
 */
void QueryCacheIntegration_Init(size_t max_entries);

/**
 * Shutdown and free the global query cache.
 */
void QueryCacheIntegration_Shutdown(void);

/**
 * Get the global query cache instance.
 * 
 * @return Pointer to the global cache, or NULL if not initialized
 */
QueryCache *QueryCacheIntegration_Get(void);

/**
 * Resize the global query cache to a new maximum number of entries.
 * 
 * @param new_max_entries New maximum number of entries
 */
void QueryCacheIntegration_Resize(size_t new_max_entries);

/**
 * Clear all entries from the global query cache.
 */
void QueryCacheIntegration_Clear(void);

/**
 * Get statistics for the global query cache.
 * 
 * @return Cache statistics structure
 */
QueryCacheStats QueryCacheIntegration_GetStats(void);

/**
 * Look up a cached query result.
 * 
 * @param index_name Name of the index
 * @param query_string Query string
 * @param limit Result limit
 * @param offset Result offset
 * @param sort_params Sort parameters (can be NULL)
 * @param index_revision Current revision of the index
 * @param size_out Output parameter for the size of the cached data
 * @return Pointer to cached data, or NULL if not found
 */
const uint8_t *QueryCacheIntegration_Lookup(const char *index_name, const char *query_string,
                                             size_t limit, size_t offset, const char *sort_params,
                                             uint64_t index_revision, size_t *size_out);

/**
 * Store a query result in the cache.
 * 
 * @param index_name Name of the index
 * @param query_string Query string
 * @param limit Result limit
 * @param offset Result offset
 * @param sort_params Sort parameters (can be NULL)
 * @param index_revision Current revision of the index
 * @param data Serialized result data
 * @param data_size Size of the serialized data
 */
void QueryCacheIntegration_Store(const char *index_name, const char *query_string,
                                  size_t limit, size_t offset, const char *sort_params,
                                  uint64_t index_revision, const uint8_t *data, size_t data_size);

