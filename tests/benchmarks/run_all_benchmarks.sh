#!/bin/bash
#
# Run all performance benchmarks and report results
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "======================================"
echo "RediSearch Performance Benchmarks"
echo "======================================"
echo ""

# Check if Redis is running
if ! redis-cli ping > /dev/null 2>&1; then
    echo "Error: Redis server is not running"
    echo "Please start Redis with RediSearch module loaded"
    exit 1
fi

echo "Redis server: OK"
echo ""

# Array to track results
declare -a results
declare -a benchmarks=(
    "bench_query_cache.py:Query Cache"
    "bench_bloom_filter.py:Bloom Filter"
    "bench_numeric_tree.py:Numeric Tree"
    "bench_cursor_adaptive.py:Cursor Adaptive (parked)"
)

# Run each benchmark
for bench_info in "${benchmarks[@]}"; do
    IFS=':' read -r script name <<< "$bench_info"
    
    echo "======================================"
    echo "Running: $name"
    echo "======================================"
    
    if python3 "$script"; then
        results+=("✓ $name: PASS")
    else
        results+=("✗ $name: FAIL")
    fi
    
    echo ""
done

# Print summary
echo "======================================"
echo "Summary"
echo "======================================"
for result in "${results[@]}"; do
    echo "$result"
done
echo ""

# Exit with failure if any benchmark failed
for result in "${results[@]}"; do
    if [[ $result == ✗* ]]; then
        exit 1
    fi
done

exit 0

