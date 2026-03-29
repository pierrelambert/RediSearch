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
#include "value.h"
#include "rmutil/alloc.h"
#include "util/arr.h"
#include "doc_table.h"
#include <string.h>

#define QUERY_CACHE_FORMAT_VERSION 2u

typedef enum QueryCacheValueType {
  QueryCacheValueType_Undef = 0,
  QueryCacheValueType_Null = 1,
  QueryCacheValueType_Number = 2,
  QueryCacheValueType_String = 3,
  QueryCacheValueType_Array = 4,
  QueryCacheValueType_Map = 5,
  QueryCacheValueType_Trio = 6,
} QueryCacheValueType;

typedef struct QueryCacheBuffer {
  uint8_t *data;
  size_t len;
  size_t cap;
} QueryCacheBuffer;

typedef struct QueryCacheReader {
  const uint8_t *cur;
  const uint8_t *end;
} QueryCacheReader;

static void QueryCache_FreeResults(SearchResult **results) {
  if (!results) {
    return;
  }

  array_foreach(results, res, {
    SearchResult_Destroy(res);
    rm_free(res);
  });
  array_free(results);
}

static bool QueryCache_BufferReserve(QueryCacheBuffer *buffer, size_t extra) {
  if (extra > SIZE_MAX - buffer->len) {
    return false;
  }

  size_t required = buffer->len + extra;
  if (required <= buffer->cap) {
    return true;
  }

  size_t new_cap = buffer->cap ? buffer->cap : 128;
  while (new_cap < required) {
    if (new_cap > SIZE_MAX / 2) {
      new_cap = required;
      break;
    }
    new_cap *= 2;
  }

  uint8_t *new_data = rm_realloc(buffer->data, new_cap);
  if (!new_data) {
    return false;
  }
  buffer->data = new_data;
  buffer->cap = new_cap;
  return true;
}

static bool QueryCache_BufferAppend(QueryCacheBuffer *buffer, const void *data, size_t len) {
  if (!QueryCache_BufferReserve(buffer, len)) {
    return false;
  }
  memcpy(buffer->data + buffer->len, data, len);
  buffer->len += len;
  return true;
}

static bool QueryCache_BufferAppendU8(QueryCacheBuffer *buffer, uint8_t value) {
  return QueryCache_BufferAppend(buffer, &value, sizeof(value));
}

static bool QueryCache_BufferAppendU32(QueryCacheBuffer *buffer, uint32_t value) {
  return QueryCache_BufferAppend(buffer, &value, sizeof(value));
}

static bool QueryCache_BufferAppendU64(QueryCacheBuffer *buffer, uint64_t value) {
  return QueryCache_BufferAppend(buffer, &value, sizeof(value));
}

static bool QueryCache_BufferAppendDouble(QueryCacheBuffer *buffer, double value) {
  return QueryCache_BufferAppend(buffer, &value, sizeof(value));
}

static bool QueryCache_ReadBytes(QueryCacheReader *reader, void *dst, size_t len) {
  if ((size_t)(reader->end - reader->cur) < len) {
    return false;
  }
  memcpy(dst, reader->cur, len);
  reader->cur += len;
  return true;
}

static bool QueryCache_ReadU8(QueryCacheReader *reader, uint8_t *value) {
  return QueryCache_ReadBytes(reader, value, sizeof(*value));
}

static bool QueryCache_ReadU32(QueryCacheReader *reader, uint32_t *value) {
  return QueryCache_ReadBytes(reader, value, sizeof(*value));
}

static bool QueryCache_ReadU64(QueryCacheReader *reader, uint64_t *value) {
  return QueryCache_ReadBytes(reader, value, sizeof(*value));
}

static bool QueryCache_ReadDouble(QueryCacheReader *reader, double *value) {
  return QueryCache_ReadBytes(reader, value, sizeof(*value));
}

static bool QueryCache_SerializeValue(QueryCacheBuffer *buffer, const RSValue *value) {
  value = value ? RSValue_Dereference(value) : NULL;
  if (!value) {
    return false;
  }

  switch (RSValue_Type(value)) {
    case RSValueType_Undef:
      return QueryCache_BufferAppendU8(buffer, QueryCacheValueType_Undef);
    case RSValueType_Null:
      return QueryCache_BufferAppendU8(buffer, QueryCacheValueType_Null);
    case RSValueType_Number:
      return QueryCache_BufferAppendU8(buffer, QueryCacheValueType_Number) &&
             QueryCache_BufferAppendDouble(buffer, RSValue_Number_Get(value));
    case RSValueType_String:
    case RSValueType_RedisString: {
      size_t len = 0;
      const char *str = RSValue_StringPtrLen(value, &len);
      if (!str || len > UINT32_MAX) {
        return false;
      }
      return QueryCache_BufferAppendU8(buffer, QueryCacheValueType_String) &&
             QueryCache_BufferAppendU32(buffer, (uint32_t)len) &&
             QueryCache_BufferAppend(buffer, str, len);
    }
    case RSValueType_Array: {
      uint32_t len = RSValue_ArrayLen(value);
      if (!QueryCache_BufferAppendU8(buffer, QueryCacheValueType_Array) ||
          !QueryCache_BufferAppendU32(buffer, len)) {
        return false;
      }
      for (uint32_t i = 0; i < len; ++i) {
        if (!QueryCache_SerializeValue(buffer, RSValue_ArrayItem(value, i))) {
          return false;
        }
      }
      return true;
    }
    case RSValueType_Map: {
      uint32_t len = RSValue_Map_Len(value);
      if (!QueryCache_BufferAppendU8(buffer, QueryCacheValueType_Map) ||
          !QueryCache_BufferAppendU32(buffer, len)) {
        return false;
      }
      for (uint32_t i = 0; i < len; ++i) {
        RSValue *key = NULL;
        RSValue *val = NULL;
        RSValue_Map_GetEntry(value, i, &key, &val);
        if (!QueryCache_SerializeValue(buffer, key) || !QueryCache_SerializeValue(buffer, val)) {
          return false;
        }
      }
      return true;
    }
    case RSValueType_Trio:
      return QueryCache_BufferAppendU8(buffer, QueryCacheValueType_Trio) &&
             QueryCache_SerializeValue(buffer, RSValue_Trio_GetLeft(value)) &&
             QueryCache_SerializeValue(buffer, RSValue_Trio_GetMiddle(value)) &&
             QueryCache_SerializeValue(buffer, RSValue_Trio_GetRight(value));
    case RSValueType_Reference:
      return QueryCache_SerializeValue(buffer, RSValue_Dereference(value));
  }

  return false;
}

static void QueryCache_FreeMapBuilderEntries(RSValueMapBuilder *builder, uint32_t count) {
  if (!builder) {
    return;
  }
  for (uint32_t i = 0; i < count; ++i) {
    if (builder->entries[i].key) {
      RSValue_DecrRef(builder->entries[i].key);
    }
    if (builder->entries[i].value) {
      RSValue_DecrRef(builder->entries[i].value);
    }
  }
  rm_free(builder->entries);
  rm_free(builder);
}

static RSValue *QueryCache_DeserializeValue(QueryCacheReader *reader) {
  uint8_t type = 0;
  if (!QueryCache_ReadU8(reader, &type)) {
    return NULL;
  }

  switch ((QueryCacheValueType)type) {
    case QueryCacheValueType_Undef:
      return RSValue_NewUndefined();
    case QueryCacheValueType_Null:
      return RSValue_NewNull();
    case QueryCacheValueType_Number: {
      double number = 0;
      if (!QueryCache_ReadDouble(reader, &number)) {
        return NULL;
      }
      return RSValue_NewNumber(number);
    }
    case QueryCacheValueType_String: {
      uint32_t len = 0;
      if (!QueryCache_ReadU32(reader, &len) || (size_t)(reader->end - reader->cur) < len) {
        return NULL;
      }
      RSValue *value = RSValue_NewCopiedString((const char *)reader->cur, len);
      reader->cur += len;
      return value;
    }
    case QueryCacheValueType_Array: {
      uint32_t len = 0;
      if (!QueryCache_ReadU32(reader, &len)) {
        return NULL;
      }
      RSValue **builder = RSValue_NewArrayBuilder(len);
      if (!builder) {
        return NULL;
      }
      for (uint32_t i = 0; i < len; ++i) {
        builder[i] = QueryCache_DeserializeValue(reader);
        if (!builder[i]) {
          for (uint32_t j = 0; j < i; ++j) {
            RSValue_DecrRef(builder[j]);
          }
          rm_free(builder);
          return NULL;
        }
      }
      return RSValue_NewArrayFromBuilder(builder, len);
    }
    case QueryCacheValueType_Map: {
      uint32_t len = 0;
      if (!QueryCache_ReadU32(reader, &len)) {
        return NULL;
      }
      RSValueMapBuilder *builder = RSValue_NewMapBuilder(len);
      if (!builder) {
        return NULL;
      }
      for (uint32_t i = 0; i < len; ++i) {
        RSValue *key = QueryCache_DeserializeValue(reader);
        RSValue *val = key ? QueryCache_DeserializeValue(reader) : NULL;
        if (!key || !val) {
          if (key) {
            RSValue_DecrRef(key);
          }
          if (val) {
            RSValue_DecrRef(val);
          }
          QueryCache_FreeMapBuilderEntries(builder, i);
          return NULL;
        }
        RSValue_MapBuilderSetEntry(builder, i, key, val);
      }
      return RSValue_NewMapFromBuilder(builder);
    }
    case QueryCacheValueType_Trio: {
      RSValue *left = QueryCache_DeserializeValue(reader);
      RSValue *middle = left ? QueryCache_DeserializeValue(reader) : NULL;
      RSValue *right = middle ? QueryCache_DeserializeValue(reader) : NULL;
      if (!left || !middle || !right) {
        if (left) {
          RSValue_DecrRef(left);
        }
        if (middle) {
          RSValue_DecrRef(middle);
        }
        if (right) {
          RSValue_DecrRef(right);
        }
        return NULL;
      }
      return RSValue_NewTrio(left, middle, right);
    }
  }

  return NULL;
}

CachedDocIds *QueryCache_SerializeDocIds(SearchResult **results, size_t count,
                                         const RLookup *lookup, uint64_t total_results,
                                         size_t *size_out) {
  if (!results || !size_out) {
    return NULL;
  }

  QueryCacheBuffer buffer = {0};
  size_t data_size = 0;
  CachedDocIds *cached = NULL;
  for (size_t i = 0; i < count; ++i) {
    const SearchResult *result = results[i];
    uint32_t field_count = 0;

    if (!QueryCache_BufferAppendU64(&buffer, SearchResult_GetDocId(result)) ||
        !QueryCache_BufferAppendDouble(&buffer, SearchResult_GetScore(result)) ||
        !QueryCache_BufferAppendU8(&buffer, SearchResult_GetFlags(result))) {
      goto error;
    }

    size_t field_count_offset = buffer.len;
    if (!QueryCache_BufferAppendU32(&buffer, 0)) {
      goto error;
    }

    if (lookup) {
      const RLookupRow *row = SearchResult_GetRowData(result);
      RLOOKUP_FOREACH(key, lookup, {
        const char *name = RLookupKey_GetName(key);
        const RSValue *value;
        size_t name_len;
        if (!name) {
          continue;
        }
        value = RLookupRow_Get(key, row);
        if (!value) {
          continue;
        }
        name_len = RLookupKey_GetNameLen(key);
        if (name_len > UINT32_MAX || field_count == UINT32_MAX) {
          goto error;
        }
        if (!QueryCache_BufferAppendU32(&buffer, (uint32_t)name_len) ||
            !QueryCache_BufferAppend(&buffer, name, name_len) ||
            !QueryCache_SerializeValue(&buffer, value)) {
          goto error;
        }
        ++field_count;
      });
    }

    memcpy(buffer.data + field_count_offset, &field_count, sizeof(field_count));
  }

  if (sizeof(CachedDocIds) > SIZE_MAX - buffer.len) {
    goto error;
  }

  data_size = sizeof(CachedDocIds) + buffer.len;
  cached = rm_malloc(data_size);
  if (!cached) {
    goto error;
  }

  cached->version = QUERY_CACHE_FORMAT_VERSION;
  cached->reserved = 0;
  cached->total_results = total_results;
  cached->count = count;
  if (buffer.len) {
    memcpy(cached->payload, buffer.data, buffer.len);
  }

  rm_free(buffer.data);
  *size_out = data_size;
  return cached;

error:
  rm_free(buffer.data);
  return NULL;
}

SearchResult **QueryCache_DeserializeDocIds(const uint8_t *cached_data, size_t data_size,
                                            const DocTable *docs, RLookup *lookup,
                                            uint64_t *total_results_out) {
  if (!cached_data || data_size < sizeof(CachedDocIds) || !docs || !total_results_out) {
    return NULL;
  }

  const CachedDocIds *cached = (const CachedDocIds *)cached_data;
  if (cached->version != QUERY_CACHE_FORMAT_VERSION) {
    return NULL;
  }

  QueryCacheReader reader = {
    .cur = cached->payload,
    .end = cached_data + data_size,
  };

  SearchResult **results = array_new(SearchResult *, cached->count ? cached->count : 1);
  if (!results) {
    return NULL;
  }

  for (uint64_t i = 0; i < cached->count; ++i) {
    uint64_t raw_doc_id = 0;
    double score = 0;
    uint8_t flags = 0;
    uint32_t field_count = 0;

    if (!QueryCache_ReadU64(&reader, &raw_doc_id) ||
        !QueryCache_ReadDouble(&reader, &score) ||
        !QueryCache_ReadU8(&reader, &flags) ||
        !QueryCache_ReadU32(&reader, &field_count)) {
      goto error;
    }

    t_docId doc_id = (t_docId)raw_doc_id;
    const RSDocumentMetadata *dmd = DocTable_Borrow(docs, doc_id);
    if (!dmd) {
      goto error;
    }

    SearchResult *res = rm_malloc(sizeof(*res));
    if (!res) {
      DMD_Return(dmd);
      goto error;
    }

    *res = SearchResult_New();
    SearchResult_SetDocId(res, doc_id);
    SearchResult_SetScore(res, score);
    SearchResult_SetFlags(res, flags);
    SearchResult_SetDocumentMetadata(res, dmd);

    for (uint32_t field_index = 0; field_index < field_count; ++field_index) {
      uint32_t name_len = 0;
      RLookupKey *key = NULL;
      RSValue *value = NULL;

      if (!QueryCache_ReadU32(&reader, &name_len) || (size_t)(reader.end - reader.cur) < name_len) {
        SearchResult_Destroy(res);
        rm_free(res);
        goto error;
      }

      key = lookup ? RLookup_GetKey_ReadEx(lookup, (const char *)reader.cur, name_len, RLOOKUP_F_NOFLAGS) : NULL;
      if (!key && lookup) {
        key = RLookup_GetKey_WriteEx(lookup, (const char *)reader.cur, name_len, RLOOKUP_F_NAMEALLOC);
      }
      reader.cur += name_len;

      value = QueryCache_DeserializeValue(&reader);
      if (!value || (lookup && !key)) {
        if (value) {
          RSValue_DecrRef(value);
        }
        SearchResult_Destroy(res);
        rm_free(res);
        goto error;
      }

      if (lookup && key) {
        RLookup_WriteOwnKey(key, SearchResult_GetRowDataMut(res), value);
      } else if (value) {
        RSValue_DecrRef(value);
      }
    }

    array_append(results, res);
  }

  if (reader.cur != reader.end) {
    goto error;
  }

  *total_results_out = cached->total_results;
  return results;

error:
  QueryCache_FreeResults(results);
  return NULL;
}

bool QueryCache_ShouldCache(QEFlags reqFlags, uint64_t limit) {
  // Don't cache FT.PROFILE replies, which require live profiling metadata.
  if (reqFlags & QEXEC_F_PROFILE) {
    return false;
  }

  // Don't cache cursor queries (stateful, partial results)
  if (reqFlags & QEXEC_F_IS_CURSOR) {
    return false;
  }

  // Don't cache aggregate queries (REDUCE clauses produce different results
  // that aren't captured in the current cache key)
  if (reqFlags & QEXEC_F_IS_AGGREGATE) {
    return false;
  }

  // Don't cache score explanation replies, which cannot be replayed from the
  // serialized result shape alone.
  if (reqFlags & QEXEC_F_SEND_SCOREEXPLAIN) {
    return false;
  }

  // Don't cache unlimited queries (LIMIT 0 0 or very large limits)
  if (limit == UINT64_MAX || limit == 0) {
    return false;
  }

  return true;
}

