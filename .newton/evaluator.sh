#!/bin/bash

# Simplified Newton Loop Evaluator
# Runs tests and returns 0 only if 100% tests pass. Saves JSON for auditing.

TEST_JSON_FILE="./.newton/state/test_output.json"

# Create state directory if it doesn't exist
mkdir -p "./.newton/state"

# Run tests using the dedicated test script (suppress output, keep JSON for auditing)
echo "Running test evaluation..." >&2
./scripts/run-tests.sh -o /dev/null -j "$TEST_JSON_FILE" >/dev/null 2>&1

# Check if JSON file exists and parse passing percentage
if [ -f "$TEST_JSON_FILE" ]; then
    # Extract passing percentage from JSON
    PASSING_PERCENTAGE=$(jq -r '.test_statistics.passing_percentage // 0' "$TEST_JSON_FILE" 2>/dev/null || echo "0")

    # Return 0 only if 100% tests pass
    if [ "$PASSING_PERCENTAGE" = "100" ]; then
        echo "✅ All tests passed (100%)" >&2
        exit 0
    else
        echo "❌ Tests failed or incomplete: ${PASSING_PERCENTAGE}% pass rate" >&2
        exit 1
    fi
else
    echo "❌ Test JSON file not found" >&2
    exit 1
fi